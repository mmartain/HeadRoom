use crate::credential_store;
use crate::providers::types::{UsageSnapshot, UsageWindow};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const PROVIDER_ID: &str = "gemini";
const DISPLAY_NAME: &str = "Gemini";
const QUOTA_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota";
const LOAD_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Gemini CLI installed-app OAuth client (public; documented as embeddable).
const OAUTH_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const OAUTH_CLIENT_SECRET: &str = "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";

struct GeminiOauth {
    access_token: String,
    refresh_token: Option<String>,
    expiry_ms: Option<u64>,
}

fn oauth_creds_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let p = home.join(".gemini").join("oauth_creds.json");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_local_oauth() -> Result<Option<GeminiOauth>, String> {
    let Some(path) = oauth_creds_path() else {
        return Ok(None);
    };
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let access = value
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let Some(access_token) = access else {
        return Ok(None);
    };
    let refresh_token = value
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let expiry_ms = value
        .get("expiry_date")
        .or_else(|| value.get("expiry"))
        .and_then(|v| v.as_u64().or_else(|| v.as_i64().map(|n| n as u64)))
        .map(|n| if n < 100_000_000_000 { n * 1000 } else { n });
    Ok(Some(GeminiOauth {
        access_token,
        refresh_token,
        expiry_ms,
    }))
}

fn write_local_oauth(access: &str, refresh: Option<&str>, expires_in_secs: Option<u64>) -> Result<(), String> {
    let Some(path) = oauth_creds_path() else {
        return Ok(());
    };
    let raw = fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
    let mut value: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    if !value.is_object() {
        value = json!({});
    }
    let obj = value.as_object_mut().unwrap();
    obj.insert("access_token".into(), json!(access));
    if let Some(r) = refresh {
        obj.insert("refresh_token".into(), json!(r));
    }
    if let Some(secs) = expires_in_secs {
        obj.insert(
            "expiry_date".into(),
            json!(now_ms().saturating_add(secs.saturating_mul(1000))),
        );
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

async fn refresh_access_token(refresh_token: &str) -> Result<GeminiOauth, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", OAUTH_CLIENT_ID),
            ("client_secret", OAUTH_CLIENT_SECRET),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "Gemini token refresh HTTP {status}: {}",
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
    Ok(GeminiOauth {
        access_token: access,
        refresh_token: new_refresh,
        expiry_ms: expires_in.map(|s| now_ms().saturating_add(s.saturating_mul(1000))),
    })
}

async fn resolve_token() -> Result<Option<String>, String> {
    if let Some(override_token) = credential_store::get_secret(PROVIDER_ID, "accessToken")? {
        return Ok(Some(override_token));
    }
    let Some(mut oauth) = read_local_oauth()? else {
        return Ok(None);
    };
    let expired = oauth
        .expiry_ms
        .map(|exp| now_ms() + 30_000 >= exp)
        .unwrap_or(false);
    if expired {
        if let Some(refresh) = oauth.refresh_token.clone() {
            if let Ok(next) = refresh_access_token(&refresh).await {
                oauth = next;
            }
        }
    }
    Ok(Some(oauth.access_token))
}

async fn resolve_project(token: &str) -> Option<String> {
    if let Ok(Some(id)) = credential_store::get_secret(PROVIDER_ID, "projectId") {
        if !id.is_empty() {
            return Some(id);
        }
    }
    let client = reqwest::Client::builder().build().ok()?;
    let response = client
        .post(LOAD_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("User-Agent", "HeadRoom/0.1")
        .json(&json!({
            "metadata": {
                "ideType": "IDE_UNSPECIFIED",
                "pluginType": "GEMINI",
                "platform": "PLATFORM_UNSPECIFIED"
            }
        }))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: Value = response.json().await.ok()?;
    body.get("cloudaicompanionProject")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_u64().map(|n| n as f64))
}

fn parse_quota(body: &Value) -> UsageSnapshot {
    let Some(Value::Array(buckets)) = body.get("buckets") else {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Connected, but Gemini quota shape was unrecognized.",
        );
    };

    // Pick the most depleted Pro and Flash buckets (by remainingFraction).
    let mut pro: Option<(f64, String, Option<String>)> = None;
    let mut flash: Option<(f64, String, Option<String>)> = None;
    let mut other: Option<(f64, String, Option<String>)> = None;

    for b in buckets {
        let remaining = b.get("remainingFraction").and_then(as_f64).unwrap_or(1.0);
        let model = b
            .get("modelId")
            .and_then(|v| v.as_str())
            .unwrap_or("model")
            .to_string();
        let reset = b
            .get("resetTime")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let lower = model.to_ascii_lowercase();
        let slot = if lower.contains("pro") {
            &mut pro
        } else if lower.contains("flash") {
            &mut flash
        } else {
            &mut other
        };
        let better = match slot {
            None => true,
            Some((prev, _, _)) => remaining < *prev,
        };
        if better {
            *slot = Some((remaining, model, reset));
        }
    }

    let mut windows = Vec::new();
    for (id, label, slot) in [
        ("pro", "Pro", pro),
        ("flash", "Flash", flash),
        ("other", "Quota", other),
    ] {
        if windows.len() >= 2 {
            break;
        }
        if let Some((remaining, model, reset)) = slot {
            let used = ((1.0 - remaining) * 100.0).clamp(0.0, 100.0);
            windows.push(UsageWindow {
                id: id.into(),
                label: if id == "other" {
                    model
                } else {
                    label.into()
                },
                used_percent: Some(used),
                remaining_label: Some(format!("{:.0}% left", (remaining * 100.0).clamp(0.0, 100.0))),
                resets_at: reset,
            });
        }
    }

    if windows.is_empty() {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "No Gemini quota buckets returned. Sign in with Gemini CLI and retry.",
        );
    }
    UsageSnapshot::ok(PROVIDER_ID, DISPLAY_NAME, windows)
}

async fn fetch_quota(token: &str, project: Option<&str>) -> Result<(u16, String), String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let body = if let Some(p) = project.filter(|s| !s.is_empty()) {
        json!({ "project": p })
    } else {
        json!({})
    };
    let response = client
        .post(QUOTA_URL)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("User-Agent", "HeadRoom/0.1")
        .json(&body)
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
            "Sign in with Gemini CLI, or paste an OAuth access token in Settings.",
        );
    };

    let project = resolve_project(&token).await;
    let mut result = match fetch_quota(&token, project.as_deref()).await {
        Ok(r) => r,
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
    };

    if (result.0 == 401 || result.0 == 403)
        && credential_store::get_secret(PROVIDER_ID, "accessToken")
            .ok()
            .flatten()
            .is_none()
    {
        if let Ok(Some(oauth)) = read_local_oauth() {
            if let Some(refresh) = oauth.refresh_token {
                if let Ok(next) = refresh_access_token(&refresh).await {
                    token = next.access_token;
                    let project = resolve_project(&token).await;
                    match fetch_quota(&token, project.as_deref()).await {
                        Ok(r) => result = r,
                        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
                    }
                }
            }
        }
    }

    let (status, body_text) = result;
    if status == 401 || status == 403 {
        return UsageSnapshot::needs_auth(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Gemini session expired. Re-auth with Gemini CLI or update the token in Settings.",
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
        Ok(body) => parse_quota(&body),
        Err(e) => UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            &format!("Invalid JSON from Gemini quota: {e}"),
        ),
    }
}
