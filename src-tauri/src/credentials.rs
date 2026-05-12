//! OS keychain wrapper. All long-lived secrets (GitHub OAuth tokens,
//! PATs for self-hosted git hosts, future encryption passphrases) live
//! here.
//!
//! Backed by the `keyring` crate's native backends:
//! - macOS: Keychain
//! - Windows: Credential Manager
//! - Linux: libsecret (Secret Service)
//!
//! Credentials are namespaced by `(service, account)` so multiple users
//! / multiple hosts can coexist (one PAT per git host, plus a GitHub
//! OAuth refresh token).

use keyring::Entry;

/// Top-level service name registered with the OS keychain. Everything
/// SaveSync writes lives under this namespace so cleanup is easy.
pub const SERVICE: &str = "app.savesync.client";

#[derive(Debug, thiserror::Error)]
pub enum CredentialsError {
    #[error("keyring backend error: {0}")]
    Keyring(#[from] keyring::Error),
}

/// Persist `value` under `(SERVICE, account)`. Overwrites any existing
/// entry — callers don't need to delete first.
pub fn store(account: &str, value: &str) -> Result<(), CredentialsError> {
    let entry = Entry::new(SERVICE, account)?;
    entry.set_password(value)?;
    Ok(())
}

/// Load the value previously stored under `account`. Returns `Ok(None)`
/// when the entry doesn't exist (the typical "not connected yet" case)
/// — only actual backend failures bubble up as errors.
pub fn load(account: &str) -> Result<Option<String>, CredentialsError> {
    let entry = Entry::new(SERVICE, account)?;
    match entry.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CredentialsError::Keyring(e)),
    }
}

/// Remove the entry for `account`. No-op if it didn't exist — callers
/// can use this idempotently as part of "disconnect this machine".
pub fn delete(account: &str) -> Result<(), CredentialsError> {
    let entry = Entry::new(SERVICE, account)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(CredentialsError::Keyring(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// Each test uses a unique account name so parallel test runs don't
    /// collide on the shared keychain, and so the cleanup `delete` call
    /// has no chance of nuking a real credential.
    fn fresh_account(prefix: &str) -> String {
        format!("test-{prefix}-{}", Uuid::new_v4())
    }

    /// Skip keychain tests when the OS backend isn't available (e.g.
    /// CI runners without an unlocked login keychain). We still exercise
    /// the wrapper's logic in store/load/delete unit-ish paths.
    fn try_round_trip(account: &str, value: &str) -> Result<(), String> {
        store(account, value).map_err(|e| e.to_string())?;
        let loaded = load(account).map_err(|e| e.to_string())?;
        if loaded.as_deref() != Some(value) {
            return Err(format!("expected {value:?}, got {loaded:?}"));
        }
        delete(account).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn round_trip_store_load_delete() {
        let account = fresh_account("round-trip");
        match try_round_trip(&account, "super-secret-token-1234") {
            Ok(()) => {}
            Err(e) if e.contains("denied") || e.contains("no entry") || e.contains("not supported") => {
                eprintln!("skipping: keychain not available ({e})");
            }
            Err(e) => panic!("keychain round-trip failed: {e}"),
        }
    }

    #[test]
    fn load_missing_entry_returns_none() {
        let account = fresh_account("missing");
        // We never stored this account. load() must return Ok(None),
        // not an error.
        match load(&account) {
            Ok(None) => {}
            Ok(Some(v)) => panic!("expected None for never-stored account, got {v:?}"),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("denied") || msg.contains("not supported") {
                    eprintln!("skipping: keychain not available ({msg})");
                    return;
                }
                panic!("load failed: {msg}");
            }
        }
    }

    #[test]
    fn delete_missing_entry_is_idempotent() {
        let account = fresh_account("delete-missing");
        match delete(&account) {
            Ok(()) => {}
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("denied") || msg.contains("not supported") {
                    eprintln!("skipping: keychain not available ({msg})");
                    return;
                }
                panic!("delete on missing entry should be a no-op: {msg}");
            }
        }
    }

    #[test]
    fn overwrite_replaces_old_value() {
        let account = fresh_account("overwrite");
        let attempt = || -> Result<(), String> {
            store(&account, "v1").map_err(|e| e.to_string())?;
            store(&account, "v2").map_err(|e| e.to_string())?;
            let got = load(&account).map_err(|e| e.to_string())?;
            if got.as_deref() != Some("v2") {
                return Err(format!("expected v2, got {got:?}"));
            }
            delete(&account).map_err(|e| e.to_string())?;
            Ok(())
        };
        match attempt() {
            Ok(()) => {}
            Err(e) if e.contains("denied") || e.contains("not supported") => {
                eprintln!("skipping: keychain not available ({e})");
            }
            Err(e) => panic!("overwrite test failed: {e}"),
        }
    }
}
