mod claude;
mod codex;
mod cursor;
mod devin;
mod gemini;
pub mod types;

use types::UsageSnapshot;

pub async fn fetch_usage(provider_id: &str) -> UsageSnapshot {
    match provider_id {
        "cursor" => cursor::fetch().await,
        "codex" => codex::fetch().await,
        "claude" => claude::fetch().await,
        "gemini" => gemini::fetch().await,
        "devin" => devin::fetch().await,
        other => UsageSnapshot::error(other, other, &format!("Unknown provider '{other}'")),
    }
}
