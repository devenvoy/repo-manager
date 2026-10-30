//! GitLab.com public OAuth application settings.

use super::{OAuthProviderAdapter, OAuthProviderConfig, configured_client_id};

pub(super) struct GitLabOAuthAdapter;

impl OAuthProviderAdapter for GitLabOAuthAdapter {
    fn config(&self, provider: &str, _endpoint: &str) -> Result<OAuthProviderConfig, String> {
        let client_id = configured_client_id(provider).ok_or_else(|| {
            "Release Desk's GitLab OAuth integration is not configured in this build.".to_string()
        })?;
        Self::config_with_client_id(client_id)
    }
}

impl GitLabOAuthAdapter {
    pub(super) fn config_with_client_id(client_id: &str) -> Result<OAuthProviderConfig, String> {
        if client_id.trim().is_empty() {
            return Err("Release Desk's GitLab OAuth client ID is empty.".into());
        }
        Ok(OAuthProviderConfig {
            authorization_endpoint: "https://gitlab.com/oauth/authorize".into(),
            token_endpoint: "https://gitlab.com/oauth/token".into(),
            client_id: client_id.trim().into(),
            scopes: vec![
                "read_user".into(),
                "read_api".into(),
                "read_repository".into(),
            ],
            callback_host: "127.0.0.1".into(),
            callback_path: "/oauth/callback".into(),
            callback_port: Some(7171),
        })
    }
}
