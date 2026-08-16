use crate::credential_store;
use crate::providers::types::{UsageSnapshot, UsageWindow};
use serde_json::Value;

const PROVIDER_ID: &str = "minimax";
const DISPLAY_NAME: &str = "MiniMax";
const DEFAULT_BASE_URL: &str = "https://www.minimax.io";
const REMAINS_PATH: &str = "/v1/token_plan/remains";

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_u64().map(|n| n as f64))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn epoch_ms_to_rfc3339(ms: f64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|dt| dt.to_rfc3339())
}

/// Builds one UsageWindow from a `model_remains` row.
/// `prefix` is "current_interval" or "current_weekly"; `end_time_key` /
/// `remains_time_key` are the matching absolute / relative reset fields.
fn window_from_row(
    row: &Value,
    id: &str,
    label: &str,
    prefix: &str,
    end_time_key: &str,
    remains_time_key: &str,
) -> Option<UsageWindow> {
    let total = row
        .get(&format!("{prefix}_total_count"))
        .and_then(as_f64)
        .unwrap_or(0.0);
    let usage = row
        .get(&format!("{prefix}_usage_count"))
        .and_then(as_f64)
        .unwrap_or(0.0);
    let remaining_pct = row
        .get(&format!("{prefix}_remaining_percent"))
        .and_then(as_f64);
    let status = row
        .get(&format!("{prefix}_status"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let resets_at = row
        .get(end_time_key)
        .and_then(as_f64)
        .and_then(epoch_ms_to_rfc3339)
        .or_else(|| {
            row.get(remains_time_key)
                .and_then(as_f64)
                .and_then(|ms| epoch_ms_to_rfc3339(now_ms() as f64 + ms))
        });

    // status 3 means the window is unlimited.
    if status == 3 {
        return Some(UsageWindow {
            id: id.into(),
            label: label.into(),
            used_percent: None,
            remaining_label: Some("Unlimited".into()),
            resets_at,
        });
    }

    // The API pre-computes remaining percent; prefer it over counts.
    // Counts fall back to the official CLI semantics: usage_count is used.
    let used = match remaining_pct {
        Some(pct) => Some((100.0 - pct).clamp(0.0, 100.0)),
        None if total > 0.0 => Some((usage / total * 100.0).clamp(0.0, 100.0)),
        None => None,
    };
    let Some(used) = used else {
        return None;
    };
    Some(UsageWindow {
        id: id.into(),
        label: label.into(),
        used_percent: Some(used),
        remaining_label: Some(format!("{:.0}% left", (100.0 - used).clamp(0.0, 100.0))),
        resets_at,
    })
}

/// Pick the row representing the coding/general model class; the Token Plan
/// quota pool is shared, so its 5-hour and weekly windows are the main signal.
fn pick_model_row<'a>(rows: &'a [Value]) -> Option<&'a Value> {
    rows.iter()
        .find(|r| {
            r.get("model_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase().contains("general"))
                .unwrap_or(false)
        })
        .or_else(|| {
            rows.iter().find(|r| {
                let name = r
                    .get("model_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                name.starts_with("minimax") || name == "m*" || name.starts_with("m2") || name.starts_with("m3")
            })
        })
        .or_else(|| rows.first())
}

fn parse_usage(body: &Value) -> UsageSnapshot {
    let Some(Value::Array(rows)) = body.get("model_remains") else {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Connected, but MiniMax response shape was unrecognized.",
        );
    };
    let Some(row) = pick_model_row(rows) else {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Connected, but MiniMax returned no quota rows.",
        );
    };

    let mut windows = Vec::new();
    if let Some(w) = window_from_row(
        row,
        "5h",
        "5-hour",
        "current_interval",
        "end_time",
        "remains_time",
    ) {
        windows.push(w);
    }
    if let Some(w) = window_from_row(
        row,
        "weekly",
        "Weekly",
        "current_weekly",
        "weekly_end_time",
        "weekly_remains_time",
    ) {
        windows.push(w);
    }

    if windows.is_empty() {
        return UsageSnapshot::error(
            PROVIDER_ID,
            DISPLAY_NAME,
            "Connected, but MiniMax returned no usable quota windows.",
        );
    }
    UsageSnapshot::ok(PROVIDER_ID, DISPLAY_NAME, windows)
}

async fn fetch_remains(base_url: &str, api_key: &str) -> Result<(u16, String), String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}{}", base_url.trim_end_matches('/'), REMAINS_PATH);
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("User-Agent", "HeadRoom/0.1")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.map_err(|e| e.to_string())?;
    Ok((status, text))
}

pub async fn fetch() -> UsageSnapshot {
    let api_key = match credential_store::get_secret(PROVIDER_ID, "apiKey") {
        Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
        _ => {
            return UsageSnapshot::needs_auth(
                PROVIDER_ID,
                DISPLAY_NAME,
                "Paste your MiniMax API key in Settings (platform.minimax.io → console → plan).",
            );
        }
    };
    let base_url = credential_store::get_secret(PROVIDER_ID, "baseUrl")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    let (status, body_text) = match fetch_remains(&base_url, &api_key).await {
        Ok(r) => r,
        Err(e) => return UsageSnapshot::error(PROVIDER_ID, DISPLAY_NAME, &e),
    };
    if status == 401 || status == 403 {
        return UsageSnapshot::needs_auth(
            PROVIDER_ID,
            DISPLAY_NAME,
            "MiniMax API key rejected. Check the key in Settings.",
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
    let body: Value = match serde_json::from_str(&body_text) {
        Ok(v) => v,
        Err(e) => {
            return UsageSnapshot::error(
                PROVIDER_ID,
                DISPLAY_NAME,
                &format!("Invalid JSON from MiniMax: {e}"),
            );
        }
    };
    if let Some(resp) = body.get("base_resp") {
        let code = resp.get("status_code").and_then(|v| v.as_i64()).unwrap_or(0);
        if code != 0 {
            let msg = resp
                .get("status_msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return UsageSnapshot::error(
                PROVIDER_ID,
                DISPLAY_NAME,
                &format!("MiniMax API error: {msg}"),
            );
        }
    }
    parse_usage(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Real response shape from MiniMax's `token_plan/remains` (count-based plan).
    fn count_row() -> Value {
        json!({
            "model_name": "MiniMax-M*",
            "start_time": 1776355200000_i64,
            "end_time": 1776373200000_i64,
            "remains_time": 7151954,
            "current_interval_total_count": 1500,
            "current_interval_usage_count": 228,
            "current_weekly_total_count": 7000,
            "current_weekly_usage_count": 300,
            "weekly_start_time": 1776009600000_i64,
            "weekly_end_time": 1776614400000_i64,
            "weekly_remains_time": 248351954
        })
    }

    /// Time-based plan: counts are 0/0, remaining percent is pre-computed.
    fn time_based_row() -> Value {
        json!({
            "model_name": "general",
            "start_time": 1776355200000_i64,
            "end_time": 1776373200000_i64,
            "remains_time": 7151954,
            "current_interval_total_count": 0,
            "current_interval_usage_count": 0,
            "current_interval_remaining_percent": 99,
            "current_interval_status": 1,
            "current_weekly_total_count": 0,
            "current_weekly_usage_count": 0,
            "current_weekly_remaining_percent": 63,
            "current_weekly_status": 1,
            "weekly_start_time": 1776009600000_i64,
            "weekly_end_time": 1776614400000_i64,
            "weekly_remains_time": 248351954
        })
    }

    #[test]
    fn interval_window_from_counts() {
        let row = count_row();
        let w = window_from_row(&row, "5h", "5-hour", "current_interval", "end_time", "remains_time")
            .expect("window expected");
        assert_eq!(w.id, "5h");
        assert_eq!(w.label, "5-hour");
        let used = w.used_percent.expect("used percent expected");
        assert!((used - 15.2).abs() < 0.01, "used = {used}");
        assert_eq!(w.remaining_label.as_deref(), Some("85% left"));
        assert!(w.resets_at.is_some(), "resets_at from end_time expected");
    }

    #[test]
    fn weekly_window_from_counts() {
        let row = count_row();
        let w = window_from_row(&row, "weekly", "Weekly", "current_weekly", "weekly_end_time", "weekly_remains_time")
            .expect("window expected");
        assert_eq!(w.id, "weekly");
        let used = w.used_percent.expect("used percent expected");
        assert!((used - (300.0 / 7000.0 * 100.0)).abs() < 0.01, "used = {used}");
        assert!(w.resets_at.is_some());
    }

    #[test]
    fn remaining_percent_preferred_over_counts() {
        let row = time_based_row();
        let w = window_from_row(&row, "5h", "5-hour", "current_interval", "end_time", "remains_time")
            .expect("window expected");
        // counts are 0/0; the API's remaining percent (99) must win
        let used = w.used_percent.expect("used percent expected");
        assert!((used - 1.0).abs() < 0.01, "used = {used}");
        assert_eq!(w.remaining_label.as_deref(), Some("99% left"));
    }

    #[test]
    fn weekly_window_from_remaining_percent() {
        let row = time_based_row();
        let w = window_from_row(&row, "weekly", "Weekly", "current_weekly", "weekly_end_time", "weekly_remains_time")
            .expect("window expected");
        let used = w.used_percent.expect("used percent expected");
        assert!((used - 37.0).abs() < 0.01, "used = {used}");
    }

    #[test]
    fn unlimited_status_renders_unlimited() {
        let mut row = time_based_row();
        row["current_interval_status"] = json!(3);
        let w = window_from_row(&row, "5h", "5-hour", "current_interval", "end_time", "remains_time")
            .expect("window expected");
        assert_eq!(w.used_percent, None);
        assert_eq!(w.remaining_label.as_deref(), Some("Unlimited"));
        assert!(w.resets_at.is_some());
    }

    #[test]
    fn no_data_returns_none() {
        let row = json!({
            "model_name": "video",
            "current_interval_total_count": 0,
            "current_interval_usage_count": 0
        });
        assert!(
            window_from_row(&row, "5h", "5-hour", "current_interval", "end_time", "remains_time")
                .is_none()
        );
    }

    #[test]
    fn pick_model_row_prefers_general() {
        let rows = json!([
            {"model_name": "video"},
            {"model_name": "general"},
            {"model_name": "speech-hd"}
        ]);
        let rows = rows.as_array().unwrap();
        let picked = pick_model_row(rows).unwrap();
        assert_eq!(picked["model_name"], "general");
    }

    #[test]
    fn pick_model_row_matches_coding_models() {
        let rows = json!([
            {"model_name": "video"},
            {"model_name": "MiniMax-M*"},
            {"model_name": "image-01"}
        ]);
        let rows = rows.as_array().unwrap();
        let picked = pick_model_row(rows).unwrap();
        assert_eq!(picked["model_name"], "MiniMax-M*");
    }

    #[test]
    fn pick_model_row_falls_back_to_first() {
        let rows = json!([
            {"model_name": "speech-hd"},
            {"model_name": "video"}
        ]);
        let rows = rows.as_array().unwrap();
        let picked = pick_model_row(rows).unwrap();
        assert_eq!(picked["model_name"], "speech-hd");
    }

    #[test]
    fn parse_usage_builds_interval_and_weekly() {
        let body = json!({
            "base_resp": {"status_code": 0, "status_msg": "success"},
            "model_remains": [time_based_row()]
        });
        let snap = parse_usage(&body);
        assert_eq!(snap.status, "ok");
        assert_eq!(snap.provider_id, PROVIDER_ID);
        assert_eq!(snap.windows.len(), 2);
        assert_eq!(snap.windows[0].id, "5h");
        assert_eq!(snap.windows[1].id, "weekly");
    }

    #[test]
    fn parse_usage_missing_shape_is_error() {
        let body = json!({"base_resp": {"status_code": 0, "status_msg": "success"}});
        let snap = parse_usage(&body);
        assert_eq!(snap.status, "error");
    }

    #[test]
    fn parse_usage_empty_rows_is_error() {
        let body = json!({"model_remains": []});
        let snap = parse_usage(&body);
        assert_eq!(snap.status, "error");
    }

    #[test]
    fn epoch_ms_to_rfc3339_formats_iso() {
        let out = epoch_ms_to_rfc3339(1776373200000.0).expect("date expected");
        assert!(out.starts_with("2026-"), "unexpected date: {out}");
        assert!(out.contains('T') && out.ends_with('Z') || out.contains('+'), "not RFC3339: {out}");
    }
}
