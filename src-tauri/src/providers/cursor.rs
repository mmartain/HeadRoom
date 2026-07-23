use crate::credential_store;
use crate::providers::types::{UsageSnapshot, UsageWindow};
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const PROVIDER_ID: &str = "cursor";
const DISPLAY_NAME: &str = "Cursor";
const OAUTH_CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";

static CURSOR_FETCH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn state_db_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
    )
}

fn read_auth_from_db(path: &PathBuf) -> Result<(Option<String>, Option<String>), String> {
    if !path.exists() {
        return Ok((None, None));
    }
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "headroom-cursor-state-{}-{}.vscdb",
        std::process::id(),
        seq
    ));
    fs::copy(path, &tmp).map_err(|e| format!("Failed to copy Cursor state DB: {e}"))?;
    let result = (|| {
        let conn = Connection::open(&tmp).map_err(|e| e.to_string())?;
        let access: Option<String> = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                ["cursorAuth/accessToken"],
                |row| row.get(0),
            )
            .ok()
            .filter(|s: &String| !s.is_empty());
        let refresh: Option<String> = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = ?1",
                ["cursorAuth/refreshToken"],
                |row| row.get(0),
            )
            .ok()
            .filter(|s: &String| !s.is_empty());
        Ok((access, refresh))
    })();
    let _ = fs::remove_file(&tmp);
    result
}

fn jwt_sub(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let padded = match payload.len() % 4 {
        2 => format!("{payload}=="),
        3 => format!("{payload}="),
        _ => payload.to_string(),
    };
    let decoded = base64_url_decode(&padded)?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    value.get("sub").and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn base64_url_decode(input: &str) -> Option<Vec<u8>> {
    let mut s = input.replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    // Minimal decoder without extra crate
    fn decode_byte(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == b'=' {
            break;
        }
        let a = decode_byte(bytes[i])?;
        let b = decode_byte(bytes[i + 1])?;
        let c = if bytes[i + 2] == b'=' {
            0
        } else {
            decode_byte(bytes[i + 2])?
        };
        let d = if bytes[i + 3] == b'=' {
            0
        } else {
            decode_byte(bytes[i + 3])?
        };
        out.push((a << 2) | (b >> 4));
        if bytes[i + 2] != b'=' {
            out.push(((b & 0xf) << 4) | (c >> 2));
        }
        if bytes[i + 3] != b'=' {
            out.push(((c & 0x3) << 6) | d);
        }
        i += 4;
    }
    Some(out)
}

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_u64().map(|n| n as f64))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

async fn refresh_access_token(refresh_token: &str) -> Result<Option<String>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post("https://api2.cursor.sh/oauth/token")
        .header("Content-Type", "application/json")
        .header("User-Agent", "HeadRoom/0.1")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": OAUTH_CLIENT_ID,
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Token refresh HTTP {status}"));
    }
    if body.get("shouldLogout").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(None);
    }
    Ok(body
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string()))
}

async fn resolve_token() -> Result<(Option<String>, Option<String>), String> {
    if let Some(override_token) = credential_store::get_secret(PROVIDER_ID, "accessToken")? {
        return Ok((Some(override_token), None));
    }
    let Some(path) = state_db_path() else {
        return Ok((None, None));
    };
    read_auth_from_db(&path)
}

fn session_cookie(token: &str) -> Option<String> {
    let sub = jwt_sub(token)?;
    Some(format!("WorkosCursorSessionToken={sub}::{token}"))
}

fn parse_usage_summary(body: &Value) -> UsageSnapshot {
    let mut windows = Vec::new();
    let plan = body
        .pointer("/individualUsage/plan")
        .cloned()
        .unwrap_or_else(|| Value::Null);
    let billing_end = body
        .get("billingCycleEnd")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let total = plan
        .get("totalPercentUsed")
        .and_then(as_f64)
        .or_else(|| body.get("totalPercentUsed").and_then(as_f64));
    let auto = plan.get("autoPercentUsed").and_then(as_f64);
    let api = plan.get("apiPercentUsed").and_then(as_f64);

    if let Some(pct) = total {
        windows.push(UsageWindow {
            id: "total".into(),
            label: "Included".into(),
            used_percent: Some(pct.clamp(0.0, 100.0)),
            remaining_label: Some(format!("{:.0}% left", (100.0 - pct).clamp(0.0, 100.0))),
            resets_at: billing_end.clone(),
        });
    }
    if let Some(pct) = auto {
        windows.push(UsageWindow {
            id: "auto".into(),
            label: "Auto + Composer".into(),
            used_percent: Some(pct.clamp(0.0, 100.0)),
            remaining_label: Some(format!("{:.0}% left", (100.0 - pct).clamp(0.0, 100.0))),
            resets_at: billing_end.clone(),
        });
    }
    if let Some(pct) = api {
        windows.push(UsageWindow {
            id: "api".into(),
            label: "API".into(),
            used_percent: Some(pct.clamp(0.0, 100.0)),
            remaining_label: Some(format!("{:.0}% left", (100.0 - pct).clamp(0.0, 100.0))),
            resets_at: billing_end.clone(),
        });
    }

    if let Some(on_demand) = body.pointer("/individualUsage/onDemand") {
        if on_demand.get("enabled").and_then(|v| v.as_bool()) != Some(false) {
            let used = on_demand.get("used").and_then(as_f64);
            let limit = on_demand.get("limit").and_then(as_f64);
            let remaining = on_demand.get("remaining").and_then(as_f64);
            let used_percent = match (used, limit) {
                (Some(u), Some(l)) if l > 0.0 => Some((u / l) * 100.0),
                _ => None,
            };
            let remaining_label = match (remaining, used) {
                (Some(r), _) => Some(format!("${:.2} on-demand left", r / 100.0)),
                (_, Some(u)) => Some(format!("${:.2} on-demand spent", u / 100.0)),
                _ => None,
            };
            if used_percent.is_some() || remaining_label.is_some() {
                windows.push(UsageWindow {
                    id: "on_demand".into(),
                    label: "On-demand".into(),
                    used_percent: used_percent.map(|p| p.clamp(0.0, 100.0)),
                    remaining_label,
                    resets_at: billing_end.clone(),
                });
            }
        }
    }

    if windows.is_empty() {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Connected, but usage-summary shape was unrecognized. Cursor may have changed their API.",
        );
    }

    UsageSnapshot::ok(PROVIDER_ID, DISPLAY_NAME, windows)
}

async fn fetch_with_token(token: &str) -> Result<(u16, String), String> {
    let cookie = session_cookie(token)
        .ok_or_else(|| "Could not derive Cursor session cookie from token".to_string())?;
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get("https://cursor.com/api/usage-summary")
        .header("Cookie", cookie)
        .header("User-Agent", "HeadRoom/0.1")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body_text = response.text().await.map_err(|e| e.to_string())?;
    Ok((status, body_text))
}

fn snapshot_from_http(status: u16, body_text: &str) -> UsageSnapshot {
    if status == 401 || status == 403 {
        return UsageSnapshot::needs_auth(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Cursor session expired. Sign in again in Cursor or update the token in Settings.",
        );
    }
    if !(200..300).contains(&status) {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            &format!(
                "HTTP {status}: {}",
                body_text.chars().take(200).collect::<String>()
            ),
        );
    }
    match serde_json::from_str::<Value>(body_text) {
        Ok(body) => parse_usage_summary(&body),
        Err(e) => UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            &format!("Invalid JSON from usage-summary: {e}"),
        ),
    }
}

pub async fn fetch() -> UsageSnapshot {
    // Serialize Cursor fetches — flyout + top bar share one process and used to race
    // on the same temp copy of state.vscdb.
    let _guard = CURSOR_FETCH_LOCK.lock().await;

    let (access, refresh) = match resolve_token().await {
        Ok(t) => t,
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
    };

    let Some(access) = access else {
        return UsageSnapshot::needs_auth(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Open Cursor and sign in, or paste an access token in Settings.",
        );
    };

    let first = match fetch_with_token(&access).await {
        Ok(v) => v,
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
    };

    if first.0 == 401 || first.0 == 403 {
        if let Some(refresh) = refresh {
            match refresh_access_token(&refresh).await {
                Ok(Some(new_token)) => {
                    return match fetch_with_token(&new_token).await {
                        Ok((status, body)) => snapshot_from_http(status, &body),
                        Err(e) => UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
                    };
                }
                Ok(None) => {
                    return UsageSnapshot::needs_auth(
                        PROVIDER_ID,
                        DISPLAY_NAME,
                        "Cursor refresh token revoked. Sign in again in Cursor.",
                    );
                }
                Err(_) => {}
            }
        }
    }

    snapshot_from_http(first.0, &first.1)
}
