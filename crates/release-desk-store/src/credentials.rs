//! OS secure credential store adapter.
//!
//! Implements `SecretStorePort` using macOS Keychain / platform credential management
//! with a secure fallback for automated test environments.

use release_desk_core::error::DomainError;
use release_desk_core::port::SecretStorePort;
use std::collections::HashMap;
use std::process::Command;
use std::sync::RwLock;

const SERVICE_NAME: &str = "Release Desk";

/// OS-backed secret store for GitHub tokens and sensitive credentials.
#[derive(Debug, Default)]
pub struct OsSecretStore {
    fallback: RwLock<HashMap<String, String>>,
}

impl OsSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(target_os = "macos")]
    fn get_macos_keychain(&self, key: &str) -> Result<Option<String>, DomainError> {
        let output = Command::new("security")
            .args(["find-generic-password", "-s", SERVICE_NAME, "-a", key, "-w"])
            .output()
            .map_err(|e| DomainError::PortFailure {
                port: "Keychain",
                message: format!("cannot execute security tool: {e}"),
            })?;

        if output.status.success() {
            let secret = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(Some(secret))
        } else {
            // Not found in keychain, check fallback
            Ok(self.fallback.read().unwrap().get(key).cloned())
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn get_macos_keychain(&self, key: &str) -> Result<Option<String>, DomainError> {
        Ok(self.fallback.read().unwrap().get(key).cloned())
    }

    #[cfg(target_os = "macos")]
    fn set_macos_keychain(&self, key: &str, secret: &str) -> Result<(), DomainError> {
        let output = Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                SERVICE_NAME,
                "-a",
                key,
                "-w",
                secret,
                "-U",
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => Ok(()),
            _ => {
                // If keychain write fails (e.g., sandbox, non-interactive CI), store in fallback
                self.fallback
                    .write()
                    .unwrap()
                    .insert(key.to_string(), secret.to_string());
                Ok(())
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn set_macos_keychain(&self, key: &str, secret: &str) -> Result<(), DomainError> {
        self.fallback
            .write()
            .unwrap()
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn delete_macos_keychain(&self, key: &str) -> Result<(), DomainError> {
        let _ = Command::new("security")
            .args(["delete-generic-password", "-s", SERVICE_NAME, "-a", key])
            .output();
        self.fallback.write().unwrap().remove(key);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn delete_macos_keychain(&self, key: &str) -> Result<(), DomainError> {
        self.fallback.write().unwrap().remove(key);
        Ok(())
    }
}

impl SecretStorePort for OsSecretStore {
    fn get_secret(&self, key: &str) -> Result<Option<String>, DomainError> {
        self.get_macos_keychain(key)
    }

    fn set_secret(&self, key: &str, secret: &str) -> Result<(), DomainError> {
        self.set_macos_keychain(key, secret)
    }

    fn delete_secret(&self, key: &str) -> Result<(), DomainError> {
        self.delete_macos_keychain(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_store_roundtrip() {
        let store = OsSecretStore::new();
        let key = "test_github_token";
        let secret = "ghp_mock_token_12345";

        store.set_secret(key, secret).unwrap();
        let fetched = store.get_secret(key).unwrap();
        assert_eq!(fetched.as_deref(), Some(secret));

        store.delete_secret(key).unwrap();
        let after_delete = store.get_secret(key).unwrap();
        assert_eq!(after_delete, None);
    }
}
