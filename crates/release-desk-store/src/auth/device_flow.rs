//! GitHub OAuth Device Authorization Grant (RFC 8628).
//!
//! This is the exact authentication workflow utilized by GitHub CLI (`gh auth login`).
//! It initiates a device authorization flow, returns a one-time user code to enter at
//! `https://github.com/login/device`, and polls GitHub for the OAuth token without
//! requiring the user to manually create, copy, or paste personal access tokens.

use release_desk_core::error::DomainError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Official GitHub CLI public Client ID (registered with GitHub for OAuth Device Flow).
pub const DEFAULT_GITHUB_CLIENT_ID: &str = "178c6fc778ccc68e1d6a";

/// Response payload returned by GitHub's `https://github.com/login/device/code` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Outcome of polling GitHub's `https://github.com/login/oauth/access_token` endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePollOutcome {
    Success {
        access_token: String,
        token_type: String,
        scope: String,
    },
    Pending,
    SlowDown,
    Expired,
    AccessDenied,
    Error(String),
}

/// Service that executes the GitHub OAuth Device Authorization Flow.
#[derive(Clone)]
pub struct GitHubDeviceFlow {
    client: reqwest::blocking::Client,
    auth_base_url: String,
    api_base_url: String,
}

impl Default for GitHubDeviceFlow {
    fn default() -> Self {
        Self::new("https://github.com", "https://api.github.com")
    }
}

impl GitHubDeviceFlow {
    /// Create a new device flow client.
    pub fn new(auth_base_url: impl Into<String>, api_base_url: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("Release-Desk-Auth/1.0")
            .build()
            .unwrap_or_default();

        Self {
            client,
            auth_base_url: auth_base_url.into(),
            api_base_url: api_base_url.into(),
        }
    }

    /// Request a new device authorization code from GitHub.
    pub fn request_code(
        &self,
        client_id: &str,
        scopes: &[&str],
    ) -> Result<DeviceCodeResponse, DomainError> {
        let cid = if client_id.trim().is_empty() {
            DEFAULT_GITHUB_CLIENT_ID
        } else {
            client_id.trim()
        };

        let scope_str = if scopes.is_empty() {
            "repo read:user".to_string()
        } else {
            scopes.join(" ")
        };

        let url = format!("{}/login/device/code", self.auth_base_url);
        let resp = self
            .client
            .post(&url)
            .header("Accept", "application/json")
            .form(&[("client_id", cid), ("scope", &scope_str)])
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "github_device_auth",
                message: format!("Failed to connect to GitHub Device Auth: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(DomainError::PortFailure {
                port: "github_device_auth",
                message: format!("GitHub Device Auth error ({status}): {body}"),
            });
        }

        resp.json::<DeviceCodeResponse>()
            .map_err(|e| DomainError::PortFailure {
                port: "github_device_auth",
                message: format!("Failed to parse GitHub Device Code response: {e}"),
            })
    }

    /// Poll GitHub for the access token during the device authorization grant.
    pub fn poll_token(
        &self,
        client_id: &str,
        device_code: &str,
    ) -> Result<DevicePollOutcome, DomainError> {
        let cid = if client_id.trim().is_empty() {
            DEFAULT_GITHUB_CLIENT_ID
        } else {
            client_id.trim()
        };

        let url = format!("{}/login/oauth/access_token", self.auth_base_url);
        let resp = self
            .client
            .post(&url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", cid),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "github_device_auth",
                message: format!("Failed to poll GitHub token: {e}"),
            })?;

        let value: serde_json::Value = resp.json().map_err(|e| DomainError::PortFailure {
            port: "github_device_auth",
            message: format!("Failed to parse JSON from GitHub token response: {e}"),
        })?;

        if let Some(token) = value.get("access_token").and_then(|v| v.as_str()) {
            let token_type = value
                .get("token_type")
                .and_then(|v| v.as_str())
                .unwrap_or("bearer")
                .to_string();
            let scope = value
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            return Ok(DevicePollOutcome::Success {
                access_token: token.to_string(),
                token_type,
                scope,
            });
        }

        if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
            return match err {
                "authorization_pending" => Ok(DevicePollOutcome::Pending),
                "slow_down" => Ok(DevicePollOutcome::SlowDown),
                "expired_token" => Ok(DevicePollOutcome::Expired),
                "access_denied" => Ok(DevicePollOutcome::AccessDenied),
                other => {
                    let desc = value
                        .get("error_description")
                        .and_then(|v| v.as_str())
                        .unwrap_or(other);
                    Ok(DevicePollOutcome::Error(desc.to_string()))
                }
            };
        }

        Ok(DevicePollOutcome::Error(format!(
            "Unexpected response: {value}"
        )))
    }

    /// Fetch the authenticated GitHub user login handle (e.g. "example").
    pub fn fetch_user_login(&self, access_token: &str) -> Result<String, DomainError> {
        let url = format!("{}/user", self.api_base_url);
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", format!("Bearer {access_token}"))
            .send()
            .map_err(|e| DomainError::PortFailure {
                port: "github_user_api",
                message: format!("Failed to fetch user profile: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(DomainError::PortFailure {
                port: "github_user_api",
                message: format!("Failed to get user details from GitHub API ({status})"),
            });
        }

        let val: serde_json::Value = resp.json().map_err(|e| DomainError::PortFailure {
            port: "github_user_api",
            message: format!("Invalid JSON from user endpoint: {e}"),
        })?;

        val.get("login")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| DomainError::PortFailure {
                port: "github_user_api",
                message: "Missing login field in user profile".into(),
            })
    }
}

/// Copy text to macOS clipboard using `pbcopy`.
pub fn copy_to_macos_clipboard(text: &str) {
    #[cfg(target_os = "macos")]
    {
        use std::io::Write as _;
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_client_id() {
        assert_eq!(DEFAULT_GITHUB_CLIENT_ID, "178c6fc778ccc68e1d6a");
    }

    #[test]
    fn test_outcome_parsing_pending() {
        let json = r#"{"error":"authorization_pending","error_description":"The authorization request is still pending."}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let err = value.get("error").and_then(|v| v.as_str()).unwrap();
        assert_eq!(err, "authorization_pending");
    }

    #[test]
    fn test_outcome_parsing_success() {
        let json =
            r#"{"access_token":"gho_123456789","token_type":"bearer","scope":"repo read:user"}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let token = value.get("access_token").and_then(|v| v.as_str()).unwrap();
        assert_eq!(token, "gho_123456789");
    }
}
