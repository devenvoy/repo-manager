//! Provider-specific OAuth endpoints and scopes for the shared authorization-code flow.

mod azure;
mod forge;
mod gitlab;

use super::OAuthProviderConfig;

/// Provider boundary for OAuth endpoint rules, client requirements, and scopes.
pub trait OAuthProviderAdapter: Send + Sync {
    fn config(&self, provider: &str, endpoint: &str) -> Result<OAuthProviderConfig, String>;
}

/// Return whether Release Desk has a provider-owned public OAuth client configured at build time.
pub fn has_configured_client(provider: &str) -> bool {
    configured_client_id(provider).is_some()
}

/// Release Desk's public GitHub OAuth client ID, when included in this build.
pub fn github_client_id() -> Option<&'static str> {
    configured_client_id("github")
}

fn configured_client_id(provider: &str) -> Option<&'static str> {
    let client_id = match provider {
        "github" => option_env!("RELEASE_DESK_GITHUB_OAUTH_CLIENT_ID"),
        "gitlab" => option_env!("RELEASE_DESK_GITLAB_OAUTH_CLIENT_ID"),
        "azure_devops" => option_env!("RELEASE_DESK_AZURE_DEVOPS_OAUTH_CLIENT_ID"),
        "gitea" => option_env!("RELEASE_DESK_GITEA_OAUTH_CLIENT_ID"),
        "codeberg" => option_env!("RELEASE_DESK_CODEBERG_OAUTH_CLIENT_ID"),
        // Each self-hosted forge issues a different client ID; a single bundled ID cannot work
        // across arbitrary Forgejo and custom instances.
        _ => None,
    }?;
    (!client_id.trim().is_empty()).then_some(client_id)
}

/// Build the provider-specific OAuth settings for browser authorization.
pub fn oauth_config(provider: &str, endpoint: &str) -> Result<OAuthProviderConfig, String> {
    let adapter: &dyn OAuthProviderAdapter = match provider {
        "azure_devops" => &azure::AzureEntraOAuthAdapter,
        "gitlab" => &gitlab::GitLabOAuthAdapter,
        "gitea" | "codeberg" => &forge::ForgeOAuthAdapter,
        _ => return Err("Native OAuth sign-in is not configured for this provider.".into()),
    };
    adapter.config(provider, endpoint)
}

/// Return the validated forge origin used for OAuth and account validation.
pub fn forge_origin(provider: &str, endpoint: &str) -> Option<String> {
    forge::origin(provider, endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlab_requires_release_desk_owned_client_configuration() {
        if has_configured_client("gitlab") {
            assert!(oauth_config("gitlab", "").is_ok());
        } else {
            assert!(oauth_config("gitlab", "").is_err());
        }
    }

    #[test]
    fn bitbucket_does_not_offer_a_desktop_client_secret_flow() {
        assert!(!has_configured_client("bitbucket"));
        assert!(oauth_config("bitbucket", "").is_err());
    }

    #[test]
    fn forge_adapter_uses_release_desk_owned_client_configuration() {
        if has_configured_client("gitea") {
            assert!(oauth_config("gitea", "").is_ok());
        } else {
            assert!(oauth_config("gitea", "").is_err());
        }
    }

    #[test]
    fn adapter_configurations_use_public_pkce_clients() {
        let config =
            gitlab::GitLabOAuthAdapter::config_with_client_id("release-desk-test-id").unwrap();
        assert_eq!(config.callback_port, Some(7171));
    }

    #[test]
    fn azure_uses_the_native_loopback_redirect_and_entra_pkce() {
        let config = azure::AzureEntraOAuthAdapter::config_with_client_id(
            "release-desk-test-id",
            "https://dev.azure.com/example",
        )
        .unwrap();
        assert_eq!(config.callback_host, "localhost");
        assert_eq!(config.callback_path, "/");
        assert!(
            config
                .scopes
                .iter()
                .any(|scope| scope.ends_with("/.default"))
        );
    }

    #[test]
    fn custom_forge_oauth_requires_a_trusted_https_origin() {
        assert_eq!(
            forge_origin("forgejo", "https://git.example.com"),
            Some("https://git.example.com".into())
        );
        assert!(forge_origin("forgejo", "http://git.example.com").is_none());
        assert!(forge_origin("forgejo", "https://user@git.example.com").is_none());
    }
}
