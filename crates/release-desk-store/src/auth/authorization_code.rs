//! Public-client OAuth authorization-code flow with PKCE and loopback callback.

#[path = "oauth_callback.rs"]
mod oauth_callback;
#[path = "oauth_credential.rs"]
mod oauth_credential;

use oauth_callback::LoopbackCallback;
pub use oauth_credential::StoredOAuthCredential;
use oauth_credential::{TokenResponse, oauth_http_client, unix_time};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Provider-specific endpoints and scopes for an OAuth public client.
#[derive(Clone, Eq, PartialEq)]
pub struct OAuthProviderConfig {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub callback_host: String,
    pub callback_path: String,
    /// Use a fixed port only when the provider requires a pre-registered redirect URI.
    pub callback_port: Option<u16>,
}

/// A single-use authorization transaction. The verifier and callback state stay local.
pub struct OAuthAuthorizationFlow {
    authorization_url: String,
    redirect_uri: String,
    token_endpoint: String,
    client_id: String,
    scopes: Vec<String>,
    verifier: String,
    callback: LoopbackCallback,
}

impl OAuthAuthorizationFlow {
    /// Start a loopback listener and construct the provider authorization URL.
    pub fn start(config: OAuthProviderConfig, cancelled: Arc<AtomicBool>) -> Result<Self, String> {
        if config.client_id.trim().is_empty() {
            return Err("Release Desk's OAuth client is not configured for this provider.".into());
        }
        let authorization_endpoint = https_endpoint(&config.authorization_endpoint)?;
        let token_endpoint = https_endpoint(&config.token_endpoint)?;
        let callback_host = match config.callback_host.as_str() {
            "127.0.0.1" | "localhost" => config.callback_host,
            _ => return Err("OAuth callback host must be localhost or 127.0.0.1.".into()),
        };
        if !config.callback_path.starts_with('/')
            || config.callback_path.contains('?')
            || config.callback_path.contains('#')
            || config.callback_path.starts_with("//")
        {
            return Err("OAuth callback path is invalid.".into());
        }
        let expected_state = uuid::Uuid::new_v4().simple().to_string();
        let callback = LoopbackCallback::start(
            &callback_host,
            config.callback_path.clone(),
            config.callback_port,
            expected_state.clone(),
            cancelled,
        )?;
        let callback_path = if config.callback_path == "/" {
            ""
        } else {
            config.callback_path.as_str()
        };
        let redirect_uri = format!("http://{callback_host}:{}{callback_path}", callback.port());
        let verifier = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let challenge = base64_url_no_pad(&Sha256::digest(verifier.as_bytes()));

        let mut authorization_url = reqwest::Url::parse(&authorization_endpoint)
            .map_err(|error| format!("Invalid authorization endpoint: {error}"))?;
        authorization_url
            .query_pairs_mut()
            .append_pair("client_id", config.client_id.trim())
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("state", &expected_state);
        authorization_url
            .query_pairs_mut()
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        if !config.scopes.is_empty() {
            authorization_url
                .query_pairs_mut()
                .append_pair("scope", &config.scopes.join(" "));
        }

        Ok(Self {
            authorization_url: authorization_url.to_string(),
            redirect_uri,
            token_endpoint,
            client_id: config.client_id.trim().to_string(),
            scopes: config.scopes,
            verifier,
            callback,
        })
    }

    pub fn authorization_url(&self) -> &str {
        &self.authorization_url
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Wait for a state-verified callback and exchange its code using PKCE.
    pub fn wait_and_exchange(self) -> Result<StoredOAuthCredential, String> {
        let code = self.callback.wait_for_code()?;
        let client = oauth_http_client()?;
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("client_id", self.client_id.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
        ];
        form.push(("code_verifier", self.verifier.as_str()));
        let response = client
            .post(&self.token_endpoint)
            .form(&form)
            .send()
            .map_err(|error| format!("OAuth token request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Provider rejected OAuth token exchange (HTTP {}).",
                response.status()
            ));
        }
        let token: TokenResponse = response.json().map_err(|error| {
            format!("Provider returned an invalid OAuth token response: {error}")
        })?;
        if token.access_token.trim().is_empty() {
            return Err("Provider returned an empty OAuth access token.".into());
        }
        let expires_at_unix = token
            .expires_in
            .map(|seconds| unix_time().map(|now| now.saturating_add(seconds)))
            .transpose()?;
        Ok(StoredOAuthCredential {
            kind: "release-desk-oauth-code-v1".into(),
            token_endpoint: self.token_endpoint,
            client_id: self.client_id,
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at_unix,
            scopes: self.scopes,
        })
    }
}

fn https_endpoint(value: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(value.trim())
        .map_err(|error| format!("Invalid OAuth endpoint: {error}"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("OAuth endpoints must be clean HTTPS URLs.".into());
    }
    Ok(parsed.to_string())
}

fn base64_url_no_pad(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(first >> 2) as usize] as char);
        output.push(TABLE[(((first & 0b11) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((second & 0b1111) << 2) | (third >> 6)) as usize] as char);
        }
        if chunk.len() > 2 {
            output.push(TABLE[(third & 0b0011_1111) as usize] as char);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_encoding_uses_url_safe_unpadded_base64() {
        assert_eq!(
            base64_url_no_pad(&Sha256::digest(b"hello")),
            "LPJNul-wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ"
        );
    }

    #[test]
    fn oauth_endpoints_must_be_https_without_userinfo_or_query() {
        assert!(https_endpoint("http://git.example.com/oauth/token").is_err());
        assert!(https_endpoint("https://user@git.example.com/oauth/token").is_err());
        assert!(https_endpoint("https://git.example.com/oauth/token?next=other").is_err());
        assert!(https_endpoint("https://git.example.com/oauth/token").is_ok());
    }

    #[test]
    fn stored_oauth_credential_requires_the_release_desk_marker() {
        assert!(StoredOAuthCredential::decode("{\"access_token\":\"not ours\"}").is_none());
    }
}
