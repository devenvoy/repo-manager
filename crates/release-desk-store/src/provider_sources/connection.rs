//! Validate a provider token before it is stored as a project connection.

use super::{api_client, failure, safe_origin, safe_path};
use release_desk_core::error::DomainError;
use sha2::{Digest, Sha256};

/// Build a Keychain reference scoped to the configured provider instance.
pub fn credential_key(provider: &str, account: &str, endpoint: Option<&str>) -> String {
    let base = format!("release_desk_{provider}_{account}");
    match endpoint.filter(|value| !value.is_empty()) {
        Some(endpoint) => {
            let digest = Sha256::digest(endpoint.trim_end_matches('/').as_bytes());
            format!("{base}_{:x}", digest)
        }
        None => base,
    }
}

/// Mark OAuth-managed credentials so provider-specific Git username conventions can apply.
pub fn oauth_credential_key(provider: &str, account: &str, endpoint: Option<&str>) -> String {
    format!("oauth_{}", credential_key(provider, account, endpoint))
}

pub fn validate_token(
    provider: &str,
    token: &str,
    endpoint: Option<&str>,
) -> Result<String, DomainError> {
    if token.trim().is_empty() || token.chars().any(char::is_control) {
        return Err(failure("Enter a non-empty provider API token"));
    }
    let (url, authorization) = match provider {
        "github" => ("https://api.github.com/user".to_string(), "bearer"),
        "gitlab" => ("https://gitlab.com/api/v4/user".to_string(), "bearer"),
        "bitbucket" => ("https://api.bitbucket.org/2.0/user".to_string(), "bearer"),
        "gitea" => ("https://gitea.com/api/v1/user".to_string(), "forge"),
        "codeberg" => ("https://codeberg.org/api/v1/user".to_string(), "forge"),
        "forgejo" | "custom" => {
            let origin =
                safe_origin(endpoint.ok_or_else(|| failure("Enter a forge server address"))?)?;
            if origin.path() != "/" {
                return Err(failure("Forge address must be its HTTPS origin"));
            }
            (format!("{}api/v1/user", origin.as_str()), "forge")
        }
        "azure_devops" => {
            let origin =
                safe_origin(endpoint.ok_or_else(|| failure("Enter an Azure organization URL"))?)?;
            if origin.host_str() != Some("dev.azure.com") || safe_path(origin.path())?.len() != 1 {
                return Err(failure("Enter an Azure organization URL"));
            }
            (
                format!(
                    "{}/_apis/projects?api-version=7.1",
                    origin.as_str().trim_end_matches('/')
                ),
                "basic",
            )
        }
        _ => return Err(failure("This provider does not use an API token")),
    };
    let client = api_client()?;
    let call = client.get(&url);
    let call = match authorization {
        "forge" => call.header("Authorization", format!("token {token}")),
        "basic" => call.basic_auth("", Some(token)),
        _ => call.bearer_auth(token),
    };
    let mut response = call.send().map_err(|error| failure(error.to_string()))?;
    if provider == "azure_devops" && response.status() == reqwest::StatusCode::UNAUTHORIZED {
        response = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .map_err(|error| failure(error.to_string()))?;
    }
    if !response.status().is_success() {
        return Err(failure(format!(
            "{} rejected this token (HTTP {}). Check its scopes and expiry",
            provider,
            response.status()
        )));
    }
    if provider == "azure_devops" {
        return Ok("azure-user".into());
    }
    let value: serde_json::Value = response
        .json()
        .map_err(|error| failure(format!("Invalid provider identity response: {error}")))?;
    let login = match provider {
        "gitlab" => value.get("username"),
        "bitbucket" => value.get("nickname").or_else(|| value.get("username")),
        _ => value.get("login"),
    }
    .and_then(serde_json::Value::as_str)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| failure("Provider response did not include an account name"))?;
    Ok(login.to_string())
}

#[cfg(test)]
mod tests {
    use super::credential_key;

    #[test]
    fn credential_references_are_scoped_to_provider_instance() {
        let first = credential_key("forgejo", "alex", Some("https://git.one.example"));
        let second = credential_key("forgejo", "alex", Some("https://git.two.example"));
        assert_ne!(first, second);
        assert_eq!(
            first,
            credential_key("forgejo", "alex", Some("https://git.one.example/"))
        );
        assert_eq!(
            credential_key("github", "alex", None),
            "release_desk_github_alex"
        );
    }
}
