use crate::credential_store;
use crate::providers::types::{UsageSnapshot, UsageWindow};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const PROVIDER_ID: &str = "codex";
const DISPLAY_NAME: &str = "Codex";

#[derive(Debug)]
struct CodexAuth {
    access_token: String,
    account_id: Option<String>,
}

fn auth_json_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".codex").join("auth.json"))
}

fn read_local_auth() -> Result<Option<CodexAuth>, String> {
    let Some(path) = auth_json_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    let access_token = value
        .pointer("/tokens/access_token")
        .or_else(|| value.get("access_token"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let Some(access_token) = access_token.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    let account_id = value
        .pointer("/tokens/account_id")
        .or_else(|| value.get("account_id"))
        .or_else(|| value.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(Some(CodexAuth {
        access_token,
        account_id,
    }))
}

fn resolve_auth() -> Result<Option<CodexAuth>, String> {
    if let Some(token) = credential_store::get_secret(PROVIDER_ID, "accessToken")? {
        let account_id = credential_store::get_secret(PROVIDER_ID, "accountId")?;
        return Ok(Some(CodexAuth {
            access_token: token,
            account_id,
        }));
    }
    read_local_auth()
}

fn window_from_rate_limit(id: &str, label: &str, node: &Value) -> Option<UsageWindow> {
    if node.is_null() {
        return None;
    }
    let used_percent = node
        .get("used_percent")
        .or_else(|| node.get("usedPercent"))
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_i64().map(|n| n as f64))
                .or_else(|| v.as_u64().map(|n| n as f64))
        })
        .or_else(|| {
            node.pointer("/limit_details/used_percent")
                .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
        })?;

    let resets_at = node
        .get("reset_at")
        .or_else(|| node.get("resetAt"))
        .or_else(|| node.get("reset_after"))
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else if let Some(n) = v.as_i64() {
                chrono::DateTime::from_timestamp(n, 0).map(|dt| dt.to_rfc3339())
            } else if let Some(n) = v.as_u64() {
                chrono::DateTime::from_timestamp(n as i64, 0).map(|dt| dt.to_rfc3339())
            } else {
                None
            }
        });

    Some(UsageWindow {
        id: id.into(),
        label: label.into(),
        used_percent: Some(used_percent.clamp(0.0, 100.0)),
        remaining_label: Some(format!(
            "{:.0}% left",
            (100.0 - used_percent).clamp(0.0, 100.0)
        )),
        resets_at,
    })
}

fn label_for_window_seconds(seconds: Option<i64>, fallback: &str) -> String {
    match seconds {
        Some(18_000) => "5-hour".into(),
        Some(604_800) => "Weekly".into(),
        Some(s) if s >= 86_400 => format!("{}-day", s / 86_400),
        Some(s) if s >= 3_600 => format!("{}-hour", s / 3_600),
        _ => fallback.into(),
    }
}

fn parse_wham_usage(body: &Value) -> UsageSnapshot {
    let mut windows = Vec::new();

    // Current shape: { rate_limit: { primary_window, secondary_window } }
    if let Some(rl) = body.get("rate_limit") {
        if let Some(primary) = rl.get("primary_window") {
            let secs = primary
                .get("limit_window_seconds")
                .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|n| n as i64)));
            let label = label_for_window_seconds(secs, "Primary");
            let id = if secs == Some(604_800) {
                "weekly"
            } else if secs == Some(18_000) {
                "5h"
            } else {
                "primary"
            };
            if let Some(w) = window_from_rate_limit(id, &label, primary) {
                windows.push(w);
            }
        }
        if let Some(secondary) = rl.get("secondary_window") {
            let secs = secondary
                .get("limit_window_seconds")
                .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|n| n as i64)));
            let label = label_for_window_seconds(secs, "Weekly");
            if let Some(w) = window_from_rate_limit("weekly", &label, secondary) {
                windows.push(w);
            }
        }
    }

    // Legacy top-level windows
    if windows.is_empty() {
        if let Some(primary) = body.get("primary_window") {
            if let Some(w) = window_from_rate_limit("5h", "5-hour", primary) {
                windows.push(w);
            }
        }
        if let Some(secondary) = body.get("secondary_window") {
            if let Some(w) = window_from_rate_limit("weekly", "Weekly", secondary) {
                windows.push(w);
            }
        }
    }

    if windows.is_empty() {
        if let Some(rl) = body.get("rate_limits") {
            if let Some(p) = rl
                .get("primary_window")
                .and_then(|n| window_from_rate_limit("5h", "5-hour", n))
            {
                windows.push(p);
            }
            if let Some(s) = rl
                .get("secondary_window")
                .and_then(|n| window_from_rate_limit("weekly", "Weekly", n))
            {
                windows.push(s);
            }
        }
    }

    // Optional model-specific limits (e.g. Codex Spark)
    if let Some(Value::Array(extra)) = body.get("additional_rate_limits") {
        for (idx, item) in extra.iter().enumerate() {
            let name = item
                .get("limit_name")
                .and_then(|v| v.as_str())
                .unwrap_or("Extra limit");
            if let Some(primary) = item.pointer("/rate_limit/primary_window") {
                if let Some(w) =
                    window_from_rate_limit(&format!("extra-{idx}"), name, primary)
                {
                    windows.push(w);
                }
            }
        }
    }

    // Credits balance may be a string
    if let Some(credits_node) = body.get("credits") {
        let has = credits_node
            .get("has_credits")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let unlimited = credits_node
            .get("unlimited")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let balance = credits_node.get("balance").and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_i64().map(|n| n as f64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        });
        if unlimited {
            windows.push(UsageWindow {
                id: "credits".into(),
                label: "Credits".into(),
                used_percent: None,
                remaining_label: Some("Unlimited".into()),
                resets_at: None,
            });
        } else if has {
            if let Some(credits) = balance {
                windows.push(UsageWindow {
                    id: "credits".into(),
                    label: "Credits".into(),
                    used_percent: None,
                    remaining_label: Some(format!("{credits:.0} credits")),
                    resets_at: None,
                });
            }
        }
    }

    if windows.is_empty() {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Connected, but /wham/usage shape was unrecognized. Codex/ChatGPT may have changed their API.",
        );
    }

    UsageSnapshot::ok(PROVIDER_ID, DISPLAY_NAME, windows)
}

pub async fn fetch() -> UsageSnapshot {
    let auth = match resolve_auth() {
        Ok(a) => a,
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
    };

    let Some(auth) = auth else {
        return UsageSnapshot::needs_auth(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Sign in with Codex CLI / ChatGPT, or paste an access token in Settings.",
        );
    };

    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e.to_string()),
    };

    let mut req = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {}", auth.access_token))
        .header("User-Agent", "HeadRoom/0.1");

    if let Some(account_id) = &auth.account_id {
        req = req.header("ChatGPT-Account-Id", account_id);
    }

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return UsageSnapshot::error(
                PROVIDER_ID,
                DISPLAY_NAME,
                &format!("Network error: {e}"),
            )
        }
    };

    let status = response.status();
    let body_text = match response.text().await {
        Ok(t) => t,
        Err(e) => {
            return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &format!("Read body: {e}"))
        }
    };

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return UsageSnapshot::needs_auth(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Codex/ChatGPT session expired. Re-authenticate Codex or update the token in Settings.",
        );
    }

    if !status.is_success() {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            &format!("HTTP {status}: {}", body_text.chars().take(200).collect::<String>()),
        );
    }

    let body: Value = match serde_json::from_str(&body_text) {
        Ok(v) => v,
        Err(e) => {
            return UsageSnapshot::error(
                PROVIDER_ID,
                DISPLAY_NAME,
                &format!("Invalid JSON from wham/usage: {e}"),
            )
        }
    };

    parse_wham_usage(&body)
}
