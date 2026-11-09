//! Secrets live only in the OS keychain (SPEC §11). Never in SQLite or files.

use crate::error::AppResult;

const SERVICE: &str = "com.lectern.app";

pub const YOUTUBE_API_KEY: &str = "youtube_api_key";

fn entry(name: &str) -> AppResult<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, name)?)
}

pub fn get(name: &str) -> AppResult<Option<String>> {
    match entry(name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn set(name: &str, value: &str) -> AppResult<()> {
    entry(name)?.set_password(value)?;
    Ok(())
}

pub fn delete(name: &str) -> AppResult<()> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
