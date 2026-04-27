use anyhow::{anyhow, Result};

const SERVICE: &str = "com.charlie.hearye";

pub const ACCOUNT_GROQ: &str = "groq_api_key";
pub const ACCOUNT_ANTHROPIC: &str = "anthropic_api_key";

pub fn is_known(account: &str) -> bool {
    matches!(account, ACCOUNT_GROQ | ACCOUNT_ANTHROPIC)
}

pub fn get(account: &str) -> Result<Option<String>> {
    let entry = keyring::Entry::new(SERVICE, account)?;
    match entry.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow!(e)),
    }
}

pub fn set(account: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, account)?;
    if value.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow!(e)),
        }
    } else {
        entry.set_password(value).map_err(|e| anyhow!(e))
    }
}
