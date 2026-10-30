//! Microsoft Entra public-client settings for Azure DevOps Services.

use super::{OAuthProviderAdapter, OAuthProviderConfig, configured_client_id};

const AZURE_DEVOPS_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";

pub(super) struct AzureEntraOAuthAdapter;

impl OAuthProviderAdapter for AzureEntraOAuthAdapter {
    fn config(
        &self,
        provider: &str,
        organization_url: &str,
    ) -> Result<OAuthProviderConfig, String> {
        let client_id = configured_client_id(provider).ok_or_else(|| {
            "Release Desk's Azure DevOps OAuth integration is not configured in this build."
                .to_string()
        })?;
        Self::config_with_client_id(client_id, organization_url)
    }
}

impl AzureEntraOAuthAdapter {
    pub(super) fn config_with_client_id(
        client_id: &str,
        organization_url: &str,
    ) -> Result<OAuthProviderConfig, String> {
        if client_id.trim().is_empty() {
            return Err("Release Desk's Azure DevOps OAuth client ID is empty.".into());
        }
        let organization = reqwest::Url::parse(organization_url.trim())
            .map_err(|_| "Enter a valid Azure DevOps organization URL first.".to_string())?;
        if organization.scheme() != "https"
            || organization.host_str() != Some("dev.azure.com")
            || organization.path().trim_matches('/').is_empty()
            || organization.path().trim_matches('/').contains('/')
            || !organization.username().is_empty()
            || organization.password().is_some()
            || organization.query().is_some()
            || organization.fragment().is_some()
        {
            return Err(
                "Enter an Azure DevOps organization URL such as https://dev.azure.com/example."
                    .into(),
            );
        }
        Ok(OAuthProviderConfig {
            authorization_endpoint:
                "https://login.microsoftonline.com/organizations/oauth2/v2.0/authorize".into(),
            token_endpoint: "https://login.microsoftonline.com/organizations/oauth2/v2.0/token"
                .into(),
            client_id: client_id.trim().into(),
            scopes: vec![
                format!("{AZURE_DEVOPS_RESOURCE}/.default"),
                "offline_access".into(),
            ],
            callback_host: "localhost".into(),
            callback_path: "/".into(),
            callback_port: None,
        })
    }
}
