//! OS secure credential store adapter.
//!
//! Implements `SecretStorePort` using macOS Keychain / platform credential management
//! and a process-local fallback only on platforms without a Keychain adapter.

use release_desk_core::error::DomainError;
use release_desk_core::port::SecretStorePort;
#[cfg(not(target_os = "macos"))]
use std::collections::HashMap;
use std::process::Command;
#[cfg(not(target_os = "macos"))]
use std::sync::RwLock;

const SERVICE_NAME: &str = "Release Desk";

/// OS-backed secret store for GitHub tokens and sensitive credentials.
#[derive(Debug, Default)]
pub struct OsSecretStore {
    #[cfg(not(target_os = "macos"))]
    fallback: RwLock<HashMap<String, String>>,
}

impl OsSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attempt to discover existing Git or CLI credentials from the OS/Git credential helper.
    pub fn discover_system_git_credentials(host: &str) -> Option<(String, String)> {
        use std::io::Write as _;
        use std::process::Stdio;

        // Try `git credential fill` non-interactively
        if let Ok(mut child) = Command::new("git")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "true")
            .args(["credential", "fill"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = writeln!(stdin, "protocol=https\nhost={host}\n");
            }
            if let Ok(output) = child.wait_with_output() {
                if output.status.success() {
                    let text = String::from_utf8_lossy(&output.stdout);
                    let mut username = None;
                    let mut password = None;
                    for line in text.lines() {
                        if let Some(u) = line.strip_prefix("username=") {
                            username = Some(u.trim().to_string());
                        } else if let Some(p) = line.strip_prefix("password=") {
                            password = Some(p.trim().to_string());
                        }
                    }
                    if let (Some(u), Some(p)) = (username, password) {
                        if !u.is_empty() && !p.is_empty() {
                            return Some((u, p));
                        }
                    }
                }
            }
        }

        // For github.com, also try `gh auth token`
        if host == "github.com" {
            if let Ok(output) = Command::new("gh").args(["auth", "token"]).output() {
                if output.status.success() {
                    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !token.is_empty() {
                        let username = if let Ok(u_out) = Command::new("gh")
                            .args(["api", "user", "--jq", ".login"])
                            .output()
                        {
                            if u_out.status.success() {
                                String::from_utf8_lossy(&u_out.stdout).trim().to_string()
                            } else {
                                "github-user".to_string()
                            }
                        } else {
                            "github-user".to_string()
                        };
                        return Some((username, token));
                    }
                }
            }
        }

        None
    }

    #[cfg(target_os = "macos")]
    fn get_macos_keychain(&self, key: &str) -> Result<Option<String>, DomainError> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(SERVICE_NAME, key) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| DomainError::PortFailure {
                    port: "Keychain",
                    message: "stored credential is not valid UTF-8".into(),
                }),
            Err(error) if error.code() == -25300 => Ok(None),
            Err(error) => Err(DomainError::PortFailure {
                port: "Keychain",
                message: format!("cannot read credential: {error}"),
            }),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn get_macos_keychain(&self, key: &str) -> Result<Option<String>, DomainError> {
        Ok(self.fallback.read().unwrap().get(key).cloned())
    }

    #[cfg(target_os = "macos")]
    fn set_macos_keychain(&self, key: &str, secret: &str) -> Result<(), DomainError> {
        security_framework::passwords::set_generic_password(SERVICE_NAME, key, secret.as_bytes())
            .map_err(|error| DomainError::PortFailure {
                port: "Keychain",
                message: format!("cannot save credential: {error}"),
            })
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
        match security_framework::passwords::delete_generic_password(SERVICE_NAME, key) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(error) => Err(DomainError::PortFailure {
                port: "Keychain",
                message: format!("cannot delete credential: {error}"),
            }),
        }
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

#[cfg(all(test, not(target_os = "macos")))]
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
