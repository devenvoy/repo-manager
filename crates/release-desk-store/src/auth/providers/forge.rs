//! Gitea, Forgejo, Codeberg, and compatible forge OAuth application settings.

use super::{OAuthProviderAdapter, OAuthProviderConfig, configured_client_id};

pub(super) struct ForgeOAuthAdapter;

impl OAuthProviderAdapter for ForgeOAuthAdapter {
    fn config(&self, provider: &str, endpoint: &str) -> Result<OAuthProviderConfig, String> {
        let client_id = configured_client_id(provider).ok_or_else(|| {
            "Release Desk's OAuth integration is not configured for this forge in this build."
                .to_string()
        })?;
        Self::config_with_client_id(provider, client_id, endpoint)
    }
}

impl ForgeOAuthAdapter {
    pub(super) fn config_with_client_id(
        provider: &str,
        client_id: &str,
        endpoint: &str,
    ) -> Result<OAuthProviderConfig, String> {
        if client_id.trim().is_empty() {
            return Err("Release Desk's forge OAuth client ID is empty.".into());
        }
        let origin = origin(provider, endpoint).ok_or_else(|| {
            "Enter a valid HTTPS forge address before browser sign-in.".to_string()
        })?;
        Ok(OAuthProviderConfig {
            authorization_endpoint: format!("{origin}/login/oauth/authorize"),
            token_endpoint: format!("{origin}/login/oauth/access_token"),
            client_id: client_id.trim().into(),
            scopes: Vec::new(),
            callback_host: "127.0.0.1".into(),
            callback_path: "/oauth/callback".into(),
            callback_port: None,
        })
    }
}

pub(super) fn origin(provider: &str, endpoint: &str) -> Option<String> {
    let raw = match provider {
        "gitea" => "https://gitea.com".to_string(),
        "codeberg" => "https://codeberg.org".to_string(),
        "forgejo" | "custom" => endpoint.trim().to_string(),
        _ => return None,
    };
    let parsed = reqwest::Url::parse(&raw).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let path = parsed.path().trim_end_matches('/');
    Some(format!(
        "{}://{}{}",
        parsed.scheme(),
        parsed.host_str()?,
        path
    ))
}
