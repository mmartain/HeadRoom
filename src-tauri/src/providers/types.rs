use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub id: String,
    pub label: String,
    pub used_percent: Option<f64>,
    pub remaining_label: Option<String>,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub provider_id: String,
    pub display_name: String,
    pub status: String,
    pub windows: Vec<UsageWindow>,
    pub fetched_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl UsageSnapshot {
    pub fn ok(provider_id: &str, display_name: &str, windows: Vec<UsageWindow>) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            display_name: display_name.to_string(),
            status: "ok".to_string(),
            windows,
            fetched_at: chrono::Utc::now().to_rfc3339(),
            error_message: None,
        }
    }

    pub fn needs_auth(provider_id: &str, display_name: &str, message: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            display_name: display_name.to_string(),
            status: "needs_auth".to_string(),
            windows: vec![],
            fetched_at: chrono::Utc::now().to_rfc3339(),
            error_message: Some(message.to_string()),
        }
    }

    pub fn error(provider_id: &str, display_name: &str, message: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            display_name: display_name.to_string(),
            status: "error".to_string(),
            windows: vec![],
            fetched_at: chrono::Utc::now().to_rfc3339(),
            error_message: Some(message.to_string()),
        }
    }
}
