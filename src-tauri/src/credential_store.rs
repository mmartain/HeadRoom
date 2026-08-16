use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

const APP_DIR: &str = "headroom";
const LEGACY_APP_DIR: &str = "remaining-token-widget";

fn app_config_dir() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or("Could not resolve config directory")?;
    let dir = base.join(APP_DIR);
    let legacy = base.join(LEGACY_APP_DIR);
    if !dir.exists() && legacy.exists() {
        if fs::rename(&legacy, &dir).is_err() {
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for name in ["settings.json", "secrets.json"] {
                let from = legacy.join(name);
                let to = dir.join(name);
                if from.exists() && !to.exists() {
                    let _ = fs::copy(&from, &to);
                }
            }
        }
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn secrets_path() -> Result<PathBuf, String> {
    Ok(app_config_dir()?.join("secrets.json"))
}

fn settings_path() -> Result<PathBuf, String> {
    Ok(app_config_dir()?.join("settings.json"))
}

fn last_resets_path() -> Result<PathBuf, String> {
    Ok(app_config_dir()?.join("last_resets.json"))
}

fn read_json(path: &PathBuf) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn write_json(path: &PathBuf, value: &Value) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

pub fn get_secret(provider_id: &str, field_key: &str) -> Result<Option<String>, String> {
    let data = read_json(&secrets_path()?)?;
    Ok(data
        .get(provider_id)
        .and_then(|p| p.get(field_key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty()))
}

pub fn set_secret(provider_id: &str, field_key: &str, value: &str) -> Result<(), String> {
    let path = secrets_path()?;
    let mut data = read_json(&path)?;
    if !data.is_object() {
        data = json!({});
    }
    let obj = data.as_object_mut().unwrap();
    let entry = obj
        .entry(provider_id.to_string())
        .or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    entry
        .as_object_mut()
        .unwrap()
        .insert(field_key.to_string(), json!(value));
    write_json(&path, &data)
}

pub fn clear_secret(provider_id: &str, field_key: &str) -> Result<(), String> {
    let path = secrets_path()?;
    let mut data = read_json(&path)?;
    if let Some(obj) = data.as_object_mut() {
        if let Some(entry) = obj.get_mut(provider_id).and_then(|v| v.as_object_mut()) {
            entry.remove(field_key);
        }
    }
    write_json(&path, &data)
}

pub fn get_settings() -> Result<Value, String> {
    read_json(&settings_path()?)
}

pub fn set_settings(value: Value) -> Result<(), String> {
    let path = settings_path()?;
    let mut existing = read_json(&path)?;
    if !existing.is_object() {
        existing = json!({});
    }
    if let (Some(dst), Some(src)) = (existing.as_object_mut(), value.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
        write_json(&path, &existing)
    } else {
        write_json(&path, &value)
    }
}

/// Last-seen window reset timestamps, used to dedupe "limits reset"
/// notifications across app restarts. Key format: `<providerId>:<windowId>`.
pub fn get_last_resets() -> Result<Value, String> {
    read_json(&last_resets_path()?)
}

/// Replace semantics: the frontend owns the full record (including pruning of
/// disabled providers' windows).
pub fn set_last_resets(value: Value) -> Result<(), String> {
    let path = last_resets_path()?;
    if value.is_object() {
        write_json(&path, &value)
    } else {
        write_json(&path, &json!({}))
    }
}
