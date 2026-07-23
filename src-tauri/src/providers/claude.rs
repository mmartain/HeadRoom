use crate::credential_store;
use crate::providers::types::{UsageSnapshot, UsageWindow};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const PROVIDER_ID: &str = "claude";
const DISPLAY_NAME: &str = "Claude";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const TOKEN_URL_PRIMARY: &str = "https://platform.claude.com/v1/oauth/token";
const TOKEN_URL_LEGACY: &str = "https://console.anthropic.com/v1/oauth/token";
/// Public Claude Code OAuth client id (installed app).
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const OAUTH_BETA: &str = "oauth-2025-04-20";

#[derive(Clone)]
struct ClaudeOauth {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_ms: Option<u64>,
}

fn credentials_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        let p = PathBuf::from(dir).join(".credentials.json");
        if p.exists() {
            return Some(p);
        }
    }
    let home = dirs::home_dir()?;
    let p = home.join(".claude").join(".credentials.json");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

fn normalize_expires_at(raw: u64) -> u64 {
    // Claude Code writes epoch ms; older tools wrote seconds.
    if raw >= 100_000_000_000 {
        raw
    } else {
        raw.saturating_mul(1000)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_local_oauth() -> Result<Option<ClaudeOauth>, String> {
    let Some(path) = credentials_path() else {
        return Ok(None);
    };
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let oauth = value
        .get("claudeAiOauth")
        .cloned()
        .unwrap_or(Value::Null);
    let access = oauth
        .get("accessToken")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let Some(access_token) = access else {
        return Ok(None);
    };
    let refresh_token = oauth
        .get("refreshToken")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let expires_at_ms = oauth
        .get("expiresAt")
        .and_then(|v| v.as_u64().or_else(|| v.as_i64().map(|n| n as u64)))
        .map(normalize_expires_at);
    Ok(Some(ClaudeOauth {
        access_token,
        refresh_token,
        expires_at_ms,
    }))
}

fn write_local_oauth(access: &str, refresh: Option<&str>, expires_in_secs: Option<u64>) -> Result<(), String> {
    let Some(path) = credentials_path() else {
        return Ok(());
    };
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if !value.is_object() {
        value = json!({});
    }
    let obj = value.as_object_mut().unwrap();
    let entry = obj
        .entry("claudeAiOauth".to_string())
        .or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    let oauth = entry.as_object_mut().unwrap();
    oauth.insert("accessToken".into(), json!(access));
    if let Some(r) = refresh {
        oauth.insert("refreshToken".into(), json!(r));
    }
    if let Some(secs) = expires_in_secs {
        let expires_ms = now_ms().saturating_add(secs.saturating_mul(1000));
        oauth.insert("expiresAt".into(), json!(expires_ms));
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

async fn refresh_access_token(refresh_token: &str) -> Result<ClaudeOauth, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": OAUTH_CLIENT_ID,
    });
    let mut last_err = String::new();
    for url in [TOKEN_URL_PRIMARY, TOKEN_URL_LEGACY] {
        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("anthropic-beta", OAUTH_BETA)
            .header("User-Agent", "HeadRoom/0.1")
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        let text = response.text().await.map_err(|e| e.to_string())?;
        if status.as_u16() == 404 || status.as_u16() == 405 {
            last_err = format!("HTTP {status} from {url}");
            continue;
        }
        if !status.is_success() {
            return Err(format!(
                "Claude token refresh HTTP {status}: {}",
                text.chars().take(120).collect::<String>()
            ));
        }
        let parsed: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let access = parsed
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Refresh response missing access_token".to_string())?
            .to_string();
        let new_refresh = parsed
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| Some(refresh_token.to_string()));
        let expires_in = parsed.get("expires_in").and_then(|v| v.as_u64());
        let _ = write_local_oauth(&access, new_refresh.as_deref(), expires_in);
        return Ok(ClaudeOauth {
            access_token: access,
            refresh_token: new_refresh,
            expires_at_ms: expires_in.map(|s| now_ms().saturating_add(s.saturating_mul(1000))),
        });
    }
    Err(last_err)
}

async fn resolve_token() -> Result<Option<String>, String> {
    if let Some(override_token) = credential_store::get_secret(PROVIDER_ID, "accessToken")? {
        return Ok(Some(override_token));
    }
    let Some(mut oauth) = read_local_oauth()? else {
        return Ok(None);
    };
    let expired = oauth
        .expires_at_ms
        .map(|exp| now_ms() + 30_000 >= exp)
        .unwrap_or(false);
    if expired {
        if let Some(refresh) = oauth.refresh_token.clone() {
            match refresh_access_token(&refresh).await {
                Ok(next) => oauth = next,
                Err(_) => {}
            }
        }
    }
    Ok(Some(oauth.access_token))
}

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_u64().map(|n| n as f64))
}

fn window_from_util(id: &str, label: &str, node: &Value) -> Option<UsageWindow> {
    if node.is_null() {
        return None;
    }
    let used = node.get("utilization").and_then(as_f64).unwrap_or(0.0);
    let resets_at = node
        .get("resets_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some(UsageWindow {
        id: id.into(),
        label: label.into(),
        used_percent: Some(used.clamp(0.0, 100.0)),
        remaining_label: Some(format!("{:.0}% left", (100.0 - used).clamp(0.0, 100.0))),
        resets_at,
    })
}

fn parse_usage(body: &Value) -> UsageSnapshot {
    let mut windows = Vec::new();
    if let Some(w) = body.get("five_hour").and_then(|n| window_from_util("5h", "5-hour", n)) {
        windows.push(w);
    }
    if let Some(w) = body
        .get("seven_day")
        .and_then(|n| window_from_util("weekly", "Weekly", n))
    {
        windows.push(w);
    }

    // Prefer an active scoped weekly limit as secondary signal when present.
    if windows.len() < 2 {
        if let Some(Value::Array(limits)) = body.get("limits") {
            let scoped = limits.iter().filter_map(|l| {
                let kind = l.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                if kind != "weekly_scoped" {
                    return None;
                }
                let percent = l.get("percent").and_then(as_f64)?;
                let name = l
                    .pointer("/scope/model/display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Model");
                let active = l.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
                Some((active, percent, name.to_string()))
            });
            let mut best: Option<(bool, f64, String)> = None;
            for item in scoped {
                let take = match &best {
                    None => true,
                    Some(cur) => item.0 > cur.0 || (item.0 == cur.0 && item.1 > cur.1),
                };
                if take {
                    best = Some(item);
                }
            }
            if let Some((_, percent, name)) = best {
                windows.push(UsageWindow {
                    id: "scoped".into(),
                    label: format!("{name} weekly"),
                    used_percent: Some(percent.clamp(0.0, 100.0)),
                    remaining_label: Some(format!(
                        "{:.0}% left",
                        (100.0 - percent).clamp(0.0, 100.0)
                    )),
                    resets_at: None,
                });
            }
        }
    }

    if windows.is_empty() {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Connected, but Claude usage shape was unrecognized.",
        );
    }
    UsageSnapshot::ok(PROVIDER_ID, DISPLAY_NAME, windows)
}

async fn fetch_usage(token: &str) -> Result<(u16, String), String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-beta", OAUTH_BETA)
        .header("User-Agent", "HeadRoom/0.1")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.map_err(|e| e.to_string())?;
    Ok((status, text))
}

pub async fn fetch() -> UsageSnapshot {
    let token = match resolve_token().await {
        Ok(t) => t,
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
    };
    let Some(mut token) = token else {
        return UsageSnapshot::needs_auth(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Sign in with Claude Code (/login), or paste an OAuth access token in Settings.",
        );
    };

    let first = match fetch_usage(&token).await {
        Ok(r) => r,
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
    };

    let (status, body_text) = if first.0 == 401 || first.0 == 403 {
        // Try one refresh if we have a local refresh token (not override-only).
        if credential_store::get_secret(PROVIDER_ID, "accessToken")
            .ok()
            .flatten()
            .is_none()
        {
            if let Ok(Some(oauth)) = read_local_oauth() {
                if let Some(refresh) = oauth.refresh_token {
                    if let Ok(next) = refresh_access_token(&refresh).await {
                        token = next.access_token;
                        match fetch_usage(&token).await {
                            Ok(r) => r,
                            Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
                        }
                    } else {
                        first
                    }
                } else {
                    first
                }
            } else {
                first
            }
        } else {
            first
        }
    } else {
        first
    };

    if status == 401 || status == 403 {
        return UsageSnapshot::needs_auth(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Claude session expired. Run claude /login or update the token in Settings.",
        );
    }
    if !(200..300).contains(&status) {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            &format!(
                "HTTP {status}: {}",
                body_text.chars().take(160).collect::<String>()
            ),
        );
    }
    match serde_json::from_str::<Value>(&body_text) {
        Ok(body) => parse_usage(&body),
        Err(e) => UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            &format!("Invalid JSON from Claude usage: {e}"),
        ),
    }
}
