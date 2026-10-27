//! Git provider identities, authentication schemes, and webview window specs.
//!
//! Models multi-host Git integrations across GitHub, GitLab, Atlassian Bitbucket,
//! and self-hosted instances with browser and webview fallback authentication.

use crate::error::ValidationError;
use crate::project::AccountIdentity;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported Git hosting platforms.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitProviderKind {
    GitHub,
    GitLab,
    Bitbucket,
    Custom(String),
}

impl GitProviderKind {
    /// Return a human-readable identifier for the provider.
    pub fn as_str(&self) -> &str {
        match self {
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
            Self::Bitbucket => "bitbucket",
            Self::Custom(name) => name.as_str(),
        }
    }

    /// Return the canonical default domain for this provider.
    pub fn default_host(&self) -> &str {
        match self {
            Self::GitHub => "github.com",
            Self::GitLab => "gitlab.com",
            Self::Bitbucket => "bitbucket.org",
            Self::Custom(name) => name.as_str(),
        }
    }

    /// Return the default API root for this provider.
    pub fn default_api_base(&self) -> &str {
        match self {
            Self::GitHub => "https://api.github.com",
            Self::GitLab => "https://gitlab.com/api/v4",
            Self::Bitbucket => "https://api.bitbucket.org/2.0",
            Self::Custom(_) => "https://localhost/api",
        }
    }

    /// Parse provider kind from string name.
    pub fn parse_kind(name: &str) -> Result<Self, ValidationError> {
        let trimmed = name.trim().to_lowercase();
        if trimmed.is_empty() {
            return Err(ValidationError::Empty("provider name"));
        }
        match trimmed.as_str() {
            "github" => Ok(Self::GitHub),
            "gitlab" => Ok(Self::GitLab),
            "bitbucket" | "atlassian" => Ok(Self::Bitbucket),
            other => Ok(Self::Custom(other.to_string())),
        }
    }
}

impl fmt::Display for GitProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Authentication method selected by the user.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    /// Direct Personal Access Token / API Token input.
    Token { secret: String },
    /// OAuth via system default browser with local loopback callback.
    OAuthBrowser,
    /// OAuth via custom embedded webview window when browser is absent.
    OAuthWebview,
}

/// Outcome status from an OAuth login flow.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthCallbackStatus {
    Success { code: String },
    Failure { error: String },
    Cancelled,
}

/// Specification for launching a custom webview authentication panel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WebviewWindowSpec {
    pub title: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub close_on_redirect_prefix: String,
}

impl WebviewWindowSpec {
    pub fn new(
        title: impl Into<String>,
        url: impl Into<String>,
        redirect_prefix: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            width: 600,
            height: 700,
            close_on_redirect_prefix: redirect_prefix.into(),
        }
    }
}

/// A connected account registered with a specific Git provider.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderAccount {
    pub provider: GitProviderKind,
    pub account: AccountIdentity,
    pub credential_ref_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_kinds() {
        assert_eq!(
            GitProviderKind::parse_kind("github").unwrap(),
            GitProviderKind::GitHub
        );
        assert_eq!(
            GitProviderKind::parse_kind("gitlab").unwrap(),
            GitProviderKind::GitLab
        );
        assert_eq!(
            GitProviderKind::parse_kind("bitbucket").unwrap(),
            GitProviderKind::Bitbucket
        );
        assert_eq!(
            GitProviderKind::parse_kind("atlassian").unwrap(),
            GitProviderKind::Bitbucket
        );
        assert_eq!(
            GitProviderKind::parse_kind("gitea.corp.internal").unwrap(),
            GitProviderKind::Custom("gitea.corp.internal".into())
        );
    }

    #[test]
    fn webview_spec_defaults() {
        let spec = WebviewWindowSpec::new(
            "Login",
            "https://github.com/login",
            "http://127.0.0.1/callback",
        );
        assert_eq!(spec.width, 600);
        assert_eq!(spec.height, 700);
        assert_eq!(spec.close_on_redirect_prefix, "http://127.0.0.1/callback");
    }
}
