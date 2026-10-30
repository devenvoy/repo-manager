use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(super) fn oauth_http_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Could not initialize OAuth HTTP client: {error}"))
}

/// OAuth credential metadata stored together with access and refresh tokens.
#[derive(Serialize, Deserialize)]
pub struct StoredOAuthCredential {
    pub(super) kind: String,
    pub(super) token_endpoint: String,
    pub(super) client_id: String,
    pub(super) access_token: String,
    pub(super) refresh_token: Option<String>,
    pub(super) expires_at_unix: Option<u64>,
    pub(super) scopes: Vec<String>,
}

impl StoredOAuthCredential {
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// Return the current access token, refreshing it shortly before expiry.
    pub fn resolve_access_token(&mut self) -> Result<&str, String> {
        let now = unix_time()?;
        if self
            .expires_at_unix
            .is_none_or(|expires_at| expires_at > now.saturating_add(60))
        {
            return Ok(&self.access_token);
        }
        let refresh_token = self
            .refresh_token
            .as_deref()
            .ok_or_else(|| "OAuth access expired; reconnect this account.".to_string())?;
        let client = oauth_http_client()?;
        let scope_value = self.scopes.join(" ");
        let mut refresh_form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];
        if !scope_value.is_empty() {
            refresh_form.push(("scope", scope_value.as_str()));
        }
        refresh_form.push(("client_id", self.client_id.as_str()));
        let response = client
            .post(&self.token_endpoint)
            .form(&refresh_form)
            .send()
            .map_err(|error| format!("OAuth token refresh failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Provider rejected OAuth refresh (HTTP {}). Reconnect this account.",
                response.status()
            ));
        }
        let refreshed: TokenResponse = response.json().map_err(|error| {
            format!("Provider returned an invalid OAuth refresh response: {error}")
        })?;
        if refreshed.access_token.trim().is_empty() {
            return Err("Provider returned an empty OAuth refresh token.".into());
        }
        self.access_token = refreshed.access_token;
        if refreshed.refresh_token.is_some() {
            self.refresh_token = refreshed.refresh_token;
        }
        self.expires_at_unix = refreshed
            .expires_in
            .map(|seconds| now.saturating_add(seconds));
        Ok(&self.access_token)
    }

    /// Serialize the OAuth credential for the OS secret store.
    pub fn encode(&self) -> Result<String, String> {
        serde_json::to_string(self)
            .map_err(|error| format!("Could not encode OAuth credential: {error}"))
    }

    /// Parse only credentials created by this PKCE flow.
    pub fn decode(value: &str) -> Option<Self> {
        let credential: Self = serde_json::from_str(value).ok()?;
        (credential.kind == "release-desk-oauth-code-v1").then_some(credential)
    }
}

#[derive(Deserialize)]
pub(super) struct TokenResponse {
    pub(super) access_token: String,
    pub(super) refresh_token: Option<String>,
    pub(super) expires_in: Option<u64>,
}

pub(super) fn unix_time() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| format!("System clock is before the Unix epoch: {error}"))
}
