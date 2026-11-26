use std::sync::{Mutex, OnceLock};

use thiserror::Error;

use crate::app_info::{KEYRING_OPENROUTER_USER, KEYRING_SERVICE};

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("credential store error: {0}")]
    Store(String),
}

/// Serializes access to the Windows Credential Manager entry.
/// `windows-native-keyring-store` documents that concurrent operations on the same entry across
/// threads may not sequence reliably, so all reads/writes pass through this lock.
fn secret_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Initializes the platform credential store once at startup.
pub fn init_store() -> Result<(), SecretError> {
    #[cfg(windows)]
    {
        let store = windows_native_keyring_store::Store::new()
            .map_err(|err| SecretError::Store(err.to_string()))?;
        keyring_core::set_default_store(store);
        Ok(())
    }

    #[cfg(not(windows))]
    {
        keyring_core::set_default_store(keyring_core::mock::Store::new());
        Ok(())
    }
}

fn open_entry() -> Result<keyring_core::Entry, SecretError> {
    keyring_core::Entry::new(KEYRING_SERVICE, KEYRING_OPENROUTER_USER)
        .map_err(|err| SecretError::Store(err.to_string()))
}

pub fn has_api_key() -> Result<bool, SecretError> {
    Ok(get_api_key()?.is_some())
}

pub fn get_api_key() -> Result<Option<String>, SecretError> {
    let _guard = secret_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = open_entry()?;
    match entry.get_password() {
        Ok(secret) => {
            let trimmed = secret.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(err) => Err(SecretError::Store(err.to_string())),
    }
}

pub fn set_api_key(key: &str) -> Result<(), SecretError> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return delete_api_key();
    }
    let _guard = secret_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = open_entry()?;
    entry
        .set_password(trimmed)
        .map_err(|err| SecretError::Store(err.to_string()))
}

pub fn delete_api_key() -> Result<(), SecretError> {
    let _guard = secret_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = open_entry()?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(err) => Err(SecretError::Store(err.to_string())),
    }
}
