//! Decrypt Cursor's Electron `safeStorage` tokens from `state.vscdb`.
//!
//! Recent Cursor builds store `cursorAuth/accessToken` as
//! `cursor-safe-storage:v1:` + base64(Chromium OSCrypt `v10` ciphertext).
//! On Windows the AES-256-GCM key comes from `%APPDATA%/Cursor/Local State`
//! (`os_crypt.encrypted_key`, DPAPI-wrapped).

use base64::Engine;
use std::path::PathBuf;
use std::sync::OnceLock;

const SEALED_PREFIX: &str = "cursor-safe-storage:v1:";
const DPAPI_PREFIX: &[u8] = b"DPAPI";
const V10_PREFIX: &[u8] = b"v10";

static MASTER_KEY: OnceLock<Vec<u8>> = OnceLock::new();

/// Return a usable JWT/string: plaintext pass-through, or decrypt sealed cells.
pub fn unwrap_token(stored: &str) -> Result<String, String> {
    if !stored.starts_with(SEALED_PREFIX) {
        return Ok(stored.to_string());
    }
    #[cfg(windows)]
    {
        unwrap_windows_sealed(stored)
    }
    #[cfg(not(windows))]
    {
        let _ = stored;
        Err(
            "Cursor encrypted its login tokens at rest; decrypting them is only implemented on Windows."
                .into(),
        )
    }
}

#[cfg(windows)]
fn unwrap_windows_sealed(stored: &str) -> Result<String, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    let encoded = &stored[SEALED_PREFIX.len()..];
    let blob = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("Cursor token base64 decode failed: {e}"))?;
    if blob.len() < 3 + 12 + 16 || &blob[..3] != V10_PREFIX {
        return Err("Cursor token ciphertext is not a recognized v10 envelope".into());
    }

    let key = master_key()?;
    let iv = &blob[3..15];
    let ciphertext_and_tag = &blob[15..];
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("Cursor AES key invalid: {e}"))?;
    let nonce = Nonce::from_slice(iv);
    let plain = cipher
        .decrypt(nonce, ciphertext_and_tag)
        .map_err(|_| {
            "Could not decrypt Cursor login token (safeStorage). Sign out/in in Cursor, then refresh."
                .to_string()
        })?;
    String::from_utf8(plain).map_err(|e| format!("Decrypted Cursor token was not UTF-8: {e}"))
}

#[cfg(windows)]
fn master_key() -> Result<&'static [u8], String> {
    if let Some(k) = MASTER_KEY.get() {
        return Ok(k.as_slice());
    }
    let key = load_master_key()?;
    let _ = MASTER_KEY.set(key);
    Ok(MASTER_KEY
        .get()
        .map(|k| k.as_slice())
        .expect("master key just set"))
}

#[cfg(windows)]
fn load_master_key() -> Result<Vec<u8>, String> {
    let path = local_state_path().ok_or_else(|| {
        "Could not locate Cursor Local State (needed to decrypt login tokens)".to_string()
    })?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read Cursor Local State: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("Invalid Cursor Local State JSON: {e}"))?;
    let b64 = value
        .pointer("/os_crypt/encrypted_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Cursor Local State is missing os_crypt.encrypted_key".to_string())?;
    let wrapped = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("Cursor encrypted_key base64 decode failed: {e}"))?;
    if wrapped.len() <= DPAPI_PREFIX.len() || &wrapped[..DPAPI_PREFIX.len()] != DPAPI_PREFIX {
        return Err("Cursor encrypted_key is not DPAPI-wrapped".into());
    }
    crate::credential_store::dpapi::decrypt(&wrapped[DPAPI_PREFIX.len()..])
}

fn local_state_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("Cursor").join("Local State"))
}
