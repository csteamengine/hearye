use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "com.charlie.hearye";

pub const ACCOUNT_GROQ: &str = "groq_api_key";
pub const ACCOUNT_ANTHROPIC: &str = "anthropic_api_key";

static CACHE: std::sync::LazyLock<Mutex<HashMap<String, Option<String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn is_known(account: &str) -> bool {
    matches!(account, ACCOUNT_GROQ | ACCOUNT_ANTHROPIC)
}

pub fn get(account: &str) -> Result<Option<String>> {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = cache.get(account) {
        return Ok(cached.clone());
    }
    drop(cache);

    let entry = keyring::Entry::new(SERVICE, account)?;
    let value = match entry.get_password() {
        Ok(s) => Some(s),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => return Err(anyhow!(e)),
    };

    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.insert(account.to_owned(), value.clone());
    Ok(value)
}

pub fn set(account: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, account)?;
    if value.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(anyhow!(e)),
        }
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        cache.insert(account.to_owned(), None);
    } else {
        entry.set_password(value).map_err(|e| anyhow!(e))?;
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        cache.insert(account.to_owned(), Some(value.to_owned()));
    }
    Ok(())
}
