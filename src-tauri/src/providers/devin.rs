use crate::credential_store;
use crate::fs_util;
use crate::providers::types::{UsageSnapshot, UsageWindow};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const PROVIDER_ID: &str = "devin";
const DISPLAY_NAME: &str = "Devin";
const COMPAT_VERSION: &str = "1.108.2";

const CREDIT_BALANCE_URL: &str = "https://api.devin.ai/v1/GetTeamCreditBalance";
const CREDIT_BALANCE_FALLBACK: &str = "https://server.codeium.com/api/v1/GetTeamCreditBalance";
const DEFAULT_API_SERVER: &str = "https://server.codeium.com";

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn state_db_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    for name in ["Devin", "devin"] {
        let p = PathBuf::from(&appdata)
            .join(name)
            .join("User")
            .join("globalStorage")
            .join("state.vscdb");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn credentials_toml_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        paths.push(PathBuf::from(&appdata).join("devin").join("credentials.toml"));
        paths.push(PathBuf::from(&appdata).join("Devin").join("credentials.toml"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        paths.push(PathBuf::from(local).join("devin").join("credentials.toml"));
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(
            home.join(".local")
                .join("share")
                .join("devin")
                .join("credentials.toml"),
        );
    }
    paths
}

fn parse_toml_quoted(raw: &str, key: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, rest)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let v = rest.trim();
        if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            if !inner.is_empty() {
                return Some(inner.to_string());
            }
        } else if let Some(inner) = v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            if !inner.is_empty() {
                return Some(inner.to_string());
            }
        } else if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

fn read_credentials_toml() -> Option<(String, String)> {
    for path in credentials_toml_paths() {
        if !path.exists() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let key = parse_toml_quoted(&raw, "windsurf_api_key")
            .or_else(|| parse_toml_quoted(&raw, "api_key"))
            .or_else(|| parse_toml_quoted(&raw, "apiKey"))?;
        let server = parse_toml_quoted(&raw, "api_server_url")
            .unwrap_or_else(|| DEFAULT_API_SERVER.to_string());
        return Some((key, server.trim_end_matches('/').to_string()));
    }
    None
}

fn read_auth_from_vscdb(path: &PathBuf) -> Result<Option<(String, Option<String>)>, String> {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "headroom-devin-state-{}-{}.vscdb",
        std::process::id(),
        seq
    ));
    if let Err(e) = fs_util::copy_shared_retry(path, &tmp, 4) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("Failed to copy Devin state DB: {e}"));
    }
    let result = (|| {
        let conn = Connection::open(&tmp).map_err(|e| e.to_string())?;
        let status_raw: Option<String> = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                ["windsurfAuthStatus"],
                |row| row.get(0),
            )
            .ok();
        let Some(status_raw) = status_raw.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let status: Value = serde_json::from_str(&status_raw).map_err(|e| e.to_string())?;
        let api_key = status
            .get("apiKey")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let Some(api_key) = api_key else {
            return Ok(None);
        };

        // Optional cached plan (used if live RPC fails).
        let cached: Option<String> = conn
            .prepare(
                "SELECT value FROM ItemTable WHERE key LIKE 'windsurf.reactSettings.cachedPlanInfoData%'",
            )
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(0))
                    .ok()
                    .and_then(|rows| rows.flatten().next())
            });

        Ok(Some((api_key, cached)))
    })();
    let _ = fs::remove_file(&tmp);
    result
}

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_u64().map(|n| n as f64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn as_f64_opt(v: Option<&Value>) -> Option<f64> {
    v.and_then(as_f64)
}

fn unix_to_rfc3339(secs: f64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs as i64, 0).map(|dt| dt.to_rfc3339())
}

fn windows_from_plan_status(plan_status: &Value) -> Vec<UsageWindow> {
    let plan_info = plan_status.get("planInfo").cloned().unwrap_or(Value::Null);
    let hide_daily = plan_info
        .get("hideDailyQuota")
        .and_then(|v| v.as_bool())
        .or_else(|| plan_status.get("hideDailyQuota").and_then(|v| v.as_bool()))
        .unwrap_or(false);

    let daily_remaining = as_f64_opt(
        plan_status
            .get("dailyQuotaRemainingPercent")
            .or_else(|| plan_status.get("dailyRemainingPercent")),
    );
    let weekly_remaining = as_f64_opt(
        plan_status
            .get("weeklyQuotaRemainingPercent")
            .or_else(|| plan_status.get("weeklyRemainingPercent")),
    );
    let daily_reset = as_f64_opt(
        plan_status
            .get("dailyQuotaResetAtUnix")
            .or_else(|| plan_status.get("dailyResetAtUnix")),
    )
    .and_then(unix_to_rfc3339);
    let weekly_reset = as_f64_opt(
        plan_status
            .get("weeklyQuotaResetAtUnix")
            .or_else(|| plan_status.get("weeklyResetAtUnix")),
    )
    .and_then(unix_to_rfc3339);

    let mut windows = Vec::new();
    if !hide_daily {
        if let Some(remaining) = daily_remaining {
            let used = (100.0 - remaining).clamp(0.0, 100.0);
            windows.push(UsageWindow {
                id: "daily".into(),
                label: "Daily".into(),
                used_percent: Some(used),
                remaining_label: Some(format!("{remaining:.0}% left")),
                resets_at: daily_reset.clone(),
            });
        }
    }
    if let Some(remaining) = weekly_remaining {
        let used = (100.0 - remaining).clamp(0.0, 100.0);
        windows.push(UsageWindow {
            id: "weekly".into(),
            label: "Weekly".into(),
            used_percent: Some(used),
            remaining_label: Some(format!("{remaining:.0}% left")),
            resets_at: weekly_reset,
        });
    } else if hide_daily {
        if let Some(remaining) = daily_remaining {
            let used = (100.0 - remaining).clamp(0.0, 100.0);
            windows.push(UsageWindow {
                id: "weekly".into(),
                label: "Weekly".into(),
                used_percent: Some(used),
                remaining_label: Some(format!("{remaining:.0}% left")),
                resets_at: weekly_reset.or(daily_reset),
            });
        }
    }
    windows
}

fn parse_cached_plan(raw: &str) -> Option<UsageSnapshot> {
    let body: Value = serde_json::from_str(raw).ok()?;
    // Cached blob uses camelCase remaining percent fields at the root.
    let mut plan_status = body.clone();
    if plan_status.get("dailyQuotaRemainingPercent").is_none() {
        if let Some(v) = body.get("dailyRemainingPercent") {
            plan_status
                .as_object_mut()?
                .insert("dailyQuotaRemainingPercent".into(), v.clone());
        }
    }
    if plan_status.get("weeklyQuotaRemainingPercent").is_none() {
        if let Some(v) = body.get("weeklyRemainingPercent") {
            plan_status
                .as_object_mut()?
                .insert("weeklyQuotaRemainingPercent".into(), v.clone());
        }
    }
    if plan_status.get("planInfo").is_none() {
        let mut info = serde_json::Map::new();
        if let Some(v) = body.get("planName") {
            info.insert("planName".into(), v.clone());
        }
        if let Some(v) = body.get("hideDailyQuota") {
            info.insert("hideDailyQuota".into(), v.clone());
        }
        plan_status
            .as_object_mut()?
            .insert("planInfo".into(), Value::Object(info));
    }
    let windows = windows_from_plan_status(&plan_status);
    if windows.is_empty() {
        return None;
    }
    Some(UsageSnapshot::ok(PROVIDER_ID, DISPLAY_NAME, windows))
}

fn parse_user_status(body: &Value) -> Result<UsageSnapshot, String> {
    let plan_status = body
        .pointer("/userStatus/planStatus")
        .or_else(|| body.get("planStatus"))
        .ok_or_else(|| "Devin response missing planStatus".to_string())?;
    let windows = windows_from_plan_status(plan_status);
    if windows.is_empty() {
        return Err("Devin returned no daily/weekly quota windows".into());
    }
    Ok(UsageSnapshot::ok(PROVIDER_ID, DISPLAY_NAME, windows))
}

fn parse_credit_balance(body: &Value) -> UsageSnapshot {
    let root = body.get("teamCreditBalance").unwrap_or(body);

    let used = root
        .get("creditsUsed")
        .or_else(|| root.get("usedCredits"))
        .or_else(|| root.get("used"))
        .and_then(as_f64);
    let available = root
        .get("creditsAvailable")
        .or_else(|| root.get("availableCredits"))
        .or_else(|| root.get("available"))
        .and_then(as_f64);
    let total = root
        .get("creditsTotal")
        .or_else(|| root.get("totalCredits"))
        .or_else(|| root.get("total"))
        .and_then(as_f64)
        .or_else(|| match (used, available) {
            (Some(u), Some(a)) => Some(u + a),
            _ => None,
        });

    let cycle_end = root
        .get("billingCycleEnd")
        .or_else(|| root.get("billing_cycle_end"))
        .or_else(|| body.get("billingCycleEnd"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let used_percent = match (used, total) {
        (Some(u), Some(t)) if t > 0.0 => Some((u / t) * 100.0),
        _ => None,
    };

    let remaining_label = match available {
        Some(a) => Some(format!("{a:.0} credits left")),
        None => used_percent.map(|p| format!("{:.0}% left", (100.0 - p).clamp(0.0, 100.0))),
    };

    if used_percent.is_none() && remaining_label.is_none() {
        if let Some(acus) = body
            .pointer("/billed_acus")
            .or_else(|| body.get("billedAcus"))
            .and_then(as_f64)
        {
            return UsageSnapshot::ok(
                PROVIDER_ID,
                DISPLAY_NAME,
                vec![UsageWindow {
                    id: "acus".into(),
                    label: "ACUs".into(),
                    used_percent: None,
                    remaining_label: Some(format!("{acus:.1} ACUs billed")),
                    resets_at: cycle_end,
                }],
            );
        }
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Connected, but credit balance shape was unrecognized.",
        );
    }

    UsageSnapshot::ok(
        PROVIDER_ID,
        DISPLAY_NAME,
        vec![UsageWindow {
            id: "credits".into(),
            label: "Credits".into(),
            used_percent: used_percent.map(|p| p.clamp(0.0, 100.0)),
            remaining_label,
            resets_at: cycle_end,
        }],
    )
}

async fn post_user_status(server: &str, api_key: &str) -> Result<(u16, String), String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "{}/exa.seat_management_pb.SeatManagementService/GetUserStatus",
        server.trim_end_matches('/')
    );
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("User-Agent", "HeadRoom/0.1")
        .json(&json!({
            "metadata": {
                "apiKey": api_key,
                "ideName": "devin",
                "ideVersion": COMPAT_VERSION,
                "extensionName": "devin",
                "extensionVersion": COMPAT_VERSION,
                "locale": "en",
            }
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.map_err(|e| e.to_string())?;
    Ok((status, text))
}

async fn post_team_balance(url: &str, service_key: &str) -> Result<(u16, String), String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {service_key}"))
        .header("Content-Type", "application/json")
        .header("User-Agent", "HeadRoom/0.1")
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.map_err(|e| e.to_string())?;
    Ok((status, text))
}

struct ResolvedAuth {
    api_key: String,
    server: String,
    cached_plan: Option<String>,
}

fn resolve_local_auth() -> Result<Option<ResolvedAuth>, String> {
    if let Some(path) = state_db_path() {
        // Corrupted/unreadable DB should not block CLI credentials.toml fallback.
        match read_auth_from_vscdb(&path) {
            Ok(Some((api_key, cached))) => {
                return Ok(Some(ResolvedAuth {
                    api_key,
                    server: DEFAULT_API_SERVER.to_string(),
                    cached_plan: cached,
                }));
            }
            Ok(None) | Err(_) => {}
        }
    }
    if let Some((api_key, server)) = read_credentials_toml() {
        return Ok(Some(ResolvedAuth {
            api_key,
            server,
            cached_plan: None,
        }));
    }
    Ok(None)
}

async fn fetch_personal(auth: &ResolvedAuth) -> UsageSnapshot {
    let servers = [
        auth.server.as_str(),
        DEFAULT_API_SERVER,
        "https://api.devin.ai",
    ];
    let mut last_err = String::new();
    for server in servers {
        match post_user_status(server, &auth.api_key).await {
            Ok((status, body_text)) => {
                if status == 401 || status == 403 {
                    return UsageSnapshot::needs_auth(
                        PROVIDER_ID,
                        DISPLAY_NAME,
                        "Devin session rejected. Sign in again in the Devin app.",
                    );
                }
                if !(200..300).contains(&status) {
                    last_err = format!(
                        "HTTP {status} from {server}: {}",
                        body_text.chars().take(140).collect::<String>()
                    );
                    continue;
                }
                match serde_json::from_str::<Value>(&body_text) {
                    Ok(body) => match parse_user_status(&body) {
                        Ok(snap) => return snap,
                        Err(e) => {
                            last_err = e;
                            continue;
                        }
                    },
                    Err(e) => {
                        last_err = format!("Invalid JSON from GetUserStatus: {e}");
                        continue;
                    }
                }
            }
            Err(e) => {
                last_err = format!("Network error calling {server}: {e}");
            }
        }
    }

    if let Some(cached) = &auth.cached_plan {
        if let Some(snap) = parse_cached_plan(cached) {
            return snap;
        }
    }

    UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &last_err)
}

async fn fetch_team_service_key(service_key: &str) -> UsageSnapshot {
    let mut last_err = String::new();
    for url in [CREDIT_BALANCE_URL, CREDIT_BALANCE_FALLBACK] {
        match post_team_balance(url, service_key).await {
            Ok((status, body_text)) => {
                if status == 401 || status == 403 {
                    return UsageSnapshot::needs_auth(
                        PROVIDER_ID,
                        DISPLAY_NAME,
                        "Devin service key rejected. Check the key scopes in team settings.",
                    );
                }
                if !(200..300).contains(&status) {
                    last_err = format!(
                        "HTTP {status} from {url}: {}",
                        body_text.chars().take(160).collect::<String>()
                    );
                    continue;
                }
                match serde_json::from_str::<Value>(&body_text) {
                    Ok(body) => return parse_credit_balance(&body),
                    Err(e) => {
                        last_err = format!("Invalid JSON from {url}: {e}");
                        continue;
                    }
                }
            }
            Err(e) => {
                last_err = format!("Network error calling {url}: {e}");
            }
        }
    }
    UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &last_err)
}

pub async fn fetch() -> UsageSnapshot {
    // Optional overrides from Settings.
    let access_override = credential_store::get_secret(PROVIDER_ID, "accessToken")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let service_key = credential_store::get_secret(PROVIDER_ID, "serviceKey")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());

    if let Some(api_key) = access_override {
        return fetch_personal(&ResolvedAuth {
            api_key,
            server: DEFAULT_API_SERVER.to_string(),
            cached_plan: None,
        })
        .await;
    }

    match resolve_local_auth() {
        Ok(Some(auth)) => return fetch_personal(&auth).await,
        Ok(None) => {}
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
    }

    if let Some(service_key) = service_key {
        return fetch_team_service_key(&service_key).await;
    }

    UsageSnapshot::needs_auth(
        PROVIDER_ID,
        DISPLAY_NAME,
        "Sign in to the Devin desktop app, or paste a session/API key in Settings.",
    )
}
