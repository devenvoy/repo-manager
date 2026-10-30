//! OS secure credential store adapter.
//!
//! Implements `SecretStorePort` using macOS Keychain / platform credential management
//! and a process-local fallback only on platforms without a Keychain adapter.

use release_desk_core::error::DomainError;
use release_desk_core::port::SecretStorePort;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, RwLock};

const SERVICE_NAME: &str = "Release Desk";

/// OS-backed secret store for GitHub tokens and sensitive credentials.
#[derive(Debug, Default)]
pub struct OsSecretStore {
    cache: RwLock<HashMap<String, String>>,
    #[cfg(target_os = "macos")]
    keychain_read: Mutex<()>,
    system_cache: Mutex<HashMap<String, (String, String)>>,
}

impl OsSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask Git Credential Manager for a host credential. It may open a browser and
    /// keeps token refresh and storage in Git Credential Manager's own store.
    pub fn credential_manager_account(
        host: &str,
        path: Option<&str>,
    ) -> Result<(String, String), DomainError> {
        use std::io::Write as _;
        use std::process::Stdio;

        if host.is_empty()
            || !host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            return Err(DomainError::PortFailure {
                port: "git_credential_manager",
                message: "Invalid provider host".into(),
            });
        }
        if path.is_some_and(|path| {
            path.is_empty()
                || path.contains(['\n', '\r', '\\'])
                || path.split('/').any(|part| part.is_empty() || part == "..")
        }) {
            return Err(DomainError::PortFailure {
                port: "git_credential_manager",
                message: "Invalid provider path".into(),
            });
        }
        let mut child = Command::new("git-credential-manager")
            .arg("get")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| DomainError::PortFailure {
                port: "git_credential_manager",
                message: "Install Git Credential Manager to use browser sign-in".into(),
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            let request = path.map_or_else(
                || format!("protocol=https\nhost={host}\n"),
                |path| format!("protocol=https\nhost={host}\npath={path}\n"),
            );
            writeln!(stdin, "{request}").map_err(|error| DomainError::PortFailure {
                port: "git_credential_manager",
                message: format!("Could not start credential request: {error}"),
            })?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| DomainError::PortFailure {
                port: "git_credential_manager",
                message: format!("Credential request failed: {error}"),
            })?;
        if !output.status.success() {
            return Err(DomainError::PortFailure {
                port: "git_credential_manager",
                message:
                    "Browser sign-in was cancelled or Git Credential Manager could not connect"
                        .into(),
            });
        }
        let mut username = None;
        let mut password = None;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(value) = line.strip_prefix("username=") {
                username = Some(value.to_string());
            } else if let Some(value) = line.strip_prefix("password=") {
                password = Some(value.to_string());
            }
        }
        match (username, password) {
            (Some(username), Some(password)) if !username.is_empty() && !password.is_empty() => {
                Ok((username, password))
            }
            _ => Err(DomainError::PortFailure {
                port: "git_credential_manager",
                message: "Git Credential Manager returned no usable account".into(),
            }),
        }
    }

    /// Reuse a successful system credential during this process. Run on a worker thread.
    pub fn discover_cached_system_git_credentials(&self, host: &str) -> Option<(String, String)> {
        self.cached_system_credential(host, || Self::discover_system_git_credentials(host))
    }

    fn cached_system_credential(
        &self,
        host: &str,
        discover: impl FnOnce() -> Option<(String, String)>,
    ) -> Option<(String, String)> {
        let mut cache = self.system_cache.lock().ok()?;
        if let Some(credential) = cache.get(host) {
            return Some(credential.clone());
        }
        let credential = discover()?;
        cache.insert(host.to_string(), credential.clone());
        Some(credential)
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
        if let Some(secret) = self.cache.read().unwrap().get(key).cloned() {
            return Ok(Some(secret));
        }
        let _guard = self
            .keychain_read
            .lock()
            .map_err(|_| DomainError::PortFailure {
                port: "Keychain",
                message: "credential lookup lock is unavailable".into(),
            })?;
        if let Some(secret) = self.cache.read().unwrap().get(key).cloned() {
            return Ok(Some(secret));
        }
        match get_generic_password(SERVICE_NAME, key) {
            Ok(bytes) => {
                let secret = String::from_utf8(bytes).map_err(|_| DomainError::PortFailure {
                    port: "Keychain",
                    message: "stored credential is not valid UTF-8".into(),
                })?;
                self.cache
                    .write()
                    .unwrap()
                    .insert(key.to_string(), secret.clone());
                Ok(Some(secret))
            }
            Err(error) if error.code() == -25300 => Ok(None),
            Err(error) => Err(DomainError::PortFailure {
                port: "Keychain",
                message: format!("cannot read credential: {error}"),
            }),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn get_macos_keychain(&self, key: &str) -> Result<Option<String>, DomainError> {
        Ok(self.cache.read().unwrap().get(key).cloned())
    }

    #[cfg(target_os = "macos")]
    fn set_macos_keychain(&self, key: &str, secret: &str) -> Result<(), DomainError> {
        security_framework::passwords::set_generic_password(SERVICE_NAME, key, secret.as_bytes())
            .map_err(|error| DomainError::PortFailure {
            port: "Keychain",
            message: format!("cannot save credential: {error}"),
        })?;
        self.cache
            .write()
            .unwrap()
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn set_macos_keychain(&self, key: &str, secret: &str) -> Result<(), DomainError> {
        self.cache
            .write()
            .unwrap()
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn delete_macos_keychain(&self, key: &str) -> Result<(), DomainError> {
        let result = match security_framework::passwords::delete_generic_password(SERVICE_NAME, key)
        {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(error) => Err(DomainError::PortFailure {
                port: "Keychain",
                message: format!("cannot delete credential: {error}"),
            }),
        };
        if result.is_ok() {
            self.cache.write().unwrap().remove(key);
        }
        result
    }

    #[cfg(not(target_os = "macos"))]
    fn delete_macos_keychain(&self, key: &str) -> Result<(), DomainError> {
        self.cache.write().unwrap().remove(key);
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

#[cfg(test)]
mod system_cache_tests {
    use super::OsSecretStore;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn concurrent_lookups_share_success_and_failures_can_be_retried() {
        let store = Arc::new(OsSecretStore::new());
        let calls = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let store = Arc::clone(&store);
                let calls = Arc::clone(&calls);
                scope.spawn(move || {
                    let result = store.cached_system_credential("example.com", || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Some(("user".into(), "token".into()))
                    });
                    assert_eq!(result, Some(("user".into(), "token".into())));
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            store
                .cached_system_credential("other.example", || None)
                .is_none()
        );
        assert!(
            store
                .cached_system_credential("other.example", || Some(("u".into(), "t".into())))
                .is_some()
        );
    }
}
