use base64::Engine;
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

// ---------------------------------------------------------------------------
// DPAPI helpers (Windows-only)
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod dpapi {
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    };
    use windows::Win32::Foundation::{LocalFree, HLOCAL};

    pub fn encrypt(plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let data_in = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &data_in,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut data_out,
            )
            .map_err(|e| format!("DPAPI encrypt failed: {e}"))?;
        }
        let out = unsafe {
            let bytes =
                std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
            let v = bytes.to_vec();
            let _ = LocalFree(Some(HLOCAL(data_out.pbData as *mut std::ffi::c_void)));
            v
        };
        Ok(out)
    }

    pub fn decrypt(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        let data_in = CRYPT_INTEGER_BLOB {
            cbData: ciphertext.len() as u32,
            pbData: ciphertext.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &data_in,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut data_out,
            )
            .map_err(|e| format!("DPAPI decrypt failed: {e}"))?;
        }
        let out = unsafe {
            let bytes =
                std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
            let v = bytes.to_vec();
            let _ = LocalFree(Some(HLOCAL(data_out.pbData as *mut std::ffi::c_void)));
            v
        };
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trip() {
            let plain = b"hello secret world";
            let enc = encrypt(plain).expect("encrypt");
            let dec = decrypt(&enc).expect("decrypt");
            assert_eq!(dec, plain);
        }
    }
}

fn read_secrets() -> Result<Value, String> {
    let path = secrets_path()?;
    if !path.exists() {
        return Ok(json!({}));
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    // Encrypted envelope: { "version": 1, "cipher": "dpapi", "data": "<b64>" }
    if value.get("version").and_then(|v| v.as_u64()) == Some(1)
        && value.get("data").and_then(|v| v.as_str()).is_some()
    {
        let b64 = value["data"].as_str().unwrap();
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("Base64 decode: {e}"))?;

        #[cfg(windows)]
        let plaintext = dpapi::decrypt(&ciphertext)?;
        #[cfg(not(windows))]
        let plaintext = ciphertext; // non-Windows stores plaintext

        return serde_json::from_slice(&plaintext).map_err(|e| e.to_string());
    }

    // Old plaintext format — migrate to encrypted on next write
    Ok(value)
}

fn write_secrets(value: &Value) -> Result<(), String> {
    let path = secrets_path()?;
    let plaintext = serde_json::to_vec(value).map_err(|e| e.to_string())?;

    #[cfg(windows)]
    let ciphertext = dpapi::encrypt(&plaintext)?;
    #[cfg(not(windows))]
    let ciphertext = plaintext.clone();

    let envelope = json!({
        "version": 1,
        "cipher": if cfg!(windows) { "dpapi" } else { "none" },
        "data": base64::engine::general_purpose::STANDARD.encode(&ciphertext),
    });
    write_json(&path, &envelope)
}

pub fn get_secret(provider_id: &str, field_key: &str) -> Result<Option<String>, String> {
    let data = read_secrets()?;
    Ok(data
        .get(provider_id)
        .and_then(|p| p.get(field_key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty()))
}

pub fn set_secret(provider_id: &str, field_key: &str, value: &str) -> Result<(), String> {
    let mut data = read_secrets()?;
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
    write_secrets(&data)
}

pub fn clear_secret(provider_id: &str, field_key: &str) -> Result<(), String> {
    let mut data = read_secrets()?;
    if let Some(obj) = data.as_object_mut() {
        if let Some(entry) = obj.get_mut(provider_id).and_then(|v| v.as_object_mut()) {
            entry.remove(field_key);
        }
    }
    write_secrets(&data)
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
