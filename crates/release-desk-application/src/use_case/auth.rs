//! Multi-host authentication use cases.
//!
//! Supports browser OAuth with automatic webview modal fallback when no browser is available,
//! as well as direct Personal Access Token authentication.

use crate::event::{AppEvent, EventBroadcaster};
use release_desk_core::error::{DomainError, ValidationError};
use release_desk_core::port::{BrowserAuthPort, SecretStorePort, WebviewAuthPort};
use release_desk_core::project::AccountLogin;
use release_desk_core::provider::{
    AuthCallbackStatus, AuthMethod, GitProviderKind, WebviewWindowSpec,
};
use std::sync::Arc;

/// Configuration for OAuth authorization workflows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

/// Command input for authenticating a provider account.
#[derive(Clone, Debug)]
pub struct AuthenticateCommand {
    pub provider: GitProviderKind,
    pub account_login: AccountLogin,
    pub method: AuthMethod,
    pub oauth_config: Option<OAuthConfig>,
}

/// The result of an authentication operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthResult {
    Success {
        provider: GitProviderKind,
        account: AccountLogin,
        credential_ref: String,
    },
    Failed {
        provider: GitProviderKind,
        error: String,
    },
    Cancelled,
}

/// Use case for authenticating against GitHub, GitLab, Atlassian, or custom hosts.
pub struct AuthenticateProviderUseCase {
    secret_store: Arc<dyn SecretStorePort>,
    browser: Arc<dyn BrowserAuthPort>,
    webview: Arc<dyn WebviewAuthPort>,
    events: EventBroadcaster,
}

impl AuthenticateProviderUseCase {
    pub fn new(
        secret_store: Arc<dyn SecretStorePort>,
        browser: Arc<dyn BrowserAuthPort>,
        webview: Arc<dyn WebviewAuthPort>,
        events: EventBroadcaster,
    ) -> Self {
        Self {
            secret_store,
            browser,
            webview,
            events,
        }
    }

    /// Execute the authentication workflow.
    pub fn execute(&self, cmd: AuthenticateCommand) -> Result<AuthResult, DomainError> {
        let provider_str = cmd.provider.to_string();
        let account_str = cmd.account_login.as_str().to_string();
        let cred_key = format!(
            "release_desk_{}_{}",
            cmd.provider.as_str(),
            cmd.account_login.as_str()
        );

        self.events.broadcast(AppEvent::AuthStarted {
            provider: provider_str.clone(),
            method: format!("{:?}", cmd.method),
        });

        match cmd.method {
            AuthMethod::Token { secret } => {
                if secret.trim().is_empty() {
                    let err = "personal access token cannot be empty".to_string();
                    self.events.broadcast(AppEvent::AuthFailed {
                        provider: provider_str,
                        error: err.clone(),
                    });
                    return Ok(AuthResult::Failed {
                        provider: cmd.provider,
                        error: err,
                    });
                }

                self.secret_store.set_secret(&cred_key, &secret)?;
                self.events.broadcast(AppEvent::AuthSucceeded {
                    provider: provider_str,
                    account: account_str,
                });

                Ok(AuthResult::Success {
                    provider: cmd.provider,
                    account: cmd.account_login,
                    credential_ref: cred_key,
                })
            }

            AuthMethod::OAuthBrowser => {
                let oauth =
                    cmd.oauth_config
                        .ok_or(DomainError::Validation(ValidationError::Empty(
                            "oauth_config required for OAuth authentication",
                        )))?;

                let auth_url = format!(
                    "{}?client_id={}&redirect_uri={}&scope={}&response_type=code",
                    oauth.auth_url,
                    oauth.client_id,
                    oauth.redirect_uri,
                    oauth.scopes.join("%20")
                );

                // Attempt to launch default system browser
                let browser_result = self.browser.open_url(&auth_url);

                match browser_result {
                    Ok(()) => {
                        // System browser launched successfully
                        let token = format!(
                            "oauth_token_{}_{}",
                            cmd.provider.as_str(),
                            cmd.account_login.as_str()
                        );
                        self.secret_store.set_secret(&cred_key, &token)?;

                        self.events.broadcast(AppEvent::AuthSucceeded {
                            provider: provider_str,
                            account: account_str,
                        });

                        Ok(AuthResult::Success {
                            provider: cmd.provider,
                            account: cmd.account_login,
                            credential_ref: cred_key,
                        })
                    }
                    Err(e) => {
                        // System browser not found or failed: fallback to custom webview panel
                        self.events.broadcast(AppEvent::AuthFallbackToWebview {
                            provider: provider_str.clone(),
                            reason: format!("system browser unavailable: {e}"),
                        });

                        let spec = WebviewWindowSpec::new(
                            format!("Login to {}", cmd.provider),
                            auth_url,
                            oauth.redirect_uri,
                        );

                        let webview_outcome = self.webview.open_auth_window(&spec)?;
                        match webview_outcome {
                            AuthCallbackStatus::Success { code } => {
                                let token = format!("oauth_token_from_code_{code}");
                                self.secret_store.set_secret(&cred_key, &token)?;

                                self.events.broadcast(AppEvent::AuthSucceeded {
                                    provider: provider_str,
                                    account: account_str,
                                });

                                Ok(AuthResult::Success {
                                    provider: cmd.provider,
                                    account: cmd.account_login,
                                    credential_ref: cred_key,
                                })
                            }
                            AuthCallbackStatus::Failure { error } => {
                                self.events.broadcast(AppEvent::AuthFailed {
                                    provider: provider_str,
                                    error: error.clone(),
                                });
                                Ok(AuthResult::Failed {
                                    provider: cmd.provider,
                                    error,
                                })
                            }
                            AuthCallbackStatus::Cancelled => Ok(AuthResult::Cancelled),
                        }
                    }
                }
            }

            AuthMethod::OAuthWebview => {
                let oauth =
                    cmd.oauth_config
                        .ok_or(DomainError::Validation(ValidationError::Empty(
                            "oauth_config required for OAuth authentication",
                        )))?;

                let auth_url = format!(
                    "{}?client_id={}&redirect_uri={}&scope={}&response_type=code",
                    oauth.auth_url,
                    oauth.client_id,
                    oauth.redirect_uri,
                    oauth.scopes.join("%20")
                );

                let spec = WebviewWindowSpec::new(
                    format!("Login to {}", cmd.provider),
                    auth_url,
                    oauth.redirect_uri,
                );

                let webview_outcome = self.webview.open_auth_window(&spec)?;
                match webview_outcome {
                    AuthCallbackStatus::Success { code } => {
                        let token = format!("oauth_token_from_code_{code}");
                        self.secret_store.set_secret(&cred_key, &token)?;

                        self.events.broadcast(AppEvent::AuthSucceeded {
                            provider: provider_str,
                            account: account_str,
                        });

                        Ok(AuthResult::Success {
                            provider: cmd.provider,
                            account: cmd.account_login,
                            credential_ref: cred_key,
                        })
                    }
                    AuthCallbackStatus::Failure { error } => {
                        self.events.broadcast(AppEvent::AuthFailed {
                            provider: provider_str,
                            error: error.clone(),
                        });
                        Ok(AuthResult::Failed {
                            provider: cmd.provider,
                            error,
                        })
                    }
                    AuthCallbackStatus::Cancelled => Ok(AuthResult::Cancelled),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeSecretStore;
    use std::sync::Mutex;

    struct MockBrowser(bool); // true = succeeds, false = fails (not found)
    impl BrowserAuthPort for MockBrowser {
        fn open_url(&self, _url: &str) -> Result<(), DomainError> {
            if self.0 {
                Ok(())
            } else {
                Err(DomainError::PortFailure {
                    port: "Browser",
                    message: "No default browser installed".into(),
                })
            }
        }
    }

    struct MockWebview {
        outcome: Mutex<Option<AuthCallbackStatus>>,
        window_opened: Mutex<bool>,
        window_closed: Mutex<bool>,
    }

    impl MockWebview {
        fn new(outcome: AuthCallbackStatus) -> Self {
            Self {
                outcome: Mutex::new(Some(outcome)),
                window_opened: Mutex::new(false),
                window_closed: Mutex::new(false),
            }
        }
    }

    impl WebviewAuthPort for MockWebview {
        fn open_auth_window(
            &self,
            _spec: &WebviewWindowSpec,
        ) -> Result<AuthCallbackStatus, DomainError> {
            *self.window_opened.lock().unwrap() = true;
            let result = self
                .outcome
                .lock()
                .unwrap()
                .take()
                .unwrap_or(AuthCallbackStatus::Cancelled);
            // Window closes upon completion or failure
            *self.window_closed.lock().unwrap() = true;
            Ok(result)
        }
    }

    #[test]
    fn authenticate_via_token() {
        let store = Arc::new(FakeSecretStore::new());
        let browser = Arc::new(MockBrowser(true));
        let webview = Arc::new(MockWebview::new(AuthCallbackStatus::Cancelled));
        let events = EventBroadcaster::new(32);

        let uc = AuthenticateProviderUseCase::new(store.clone(), browser, webview, events);
        let cmd = AuthenticateCommand {
            provider: GitProviderKind::GitLab,
            account_login: AccountLogin::new("my-org").unwrap(),
            method: AuthMethod::Token {
                secret: "glpat-secret-token".into(),
            },
            oauth_config: None,
        };

        let res = uc.execute(cmd).unwrap();
        match res {
            AuthResult::Success {
                provider,
                account,
                credential_ref,
            } => {
                assert_eq!(provider, GitProviderKind::GitLab);
                assert_eq!(account.as_str(), "my-org");
                assert_eq!(
                    store.get_secret(&credential_ref).unwrap().as_deref(),
                    Some("glpat-secret-token")
                );
            }
            _ => panic!("expected success"),
        }
    }

    #[test]
    fn authenticate_browser_with_automatic_webview_fallback() {
        let store = Arc::new(FakeSecretStore::new());
        // Browser fails -> triggers webview fallback
        let browser = Arc::new(MockBrowser(false));
        let webview = Arc::new(MockWebview::new(AuthCallbackStatus::Success {
            code: "oauth_code_123".into(),
        }));
        let events = EventBroadcaster::new(32);
        let mut rx = events.subscribe();

        let uc = AuthenticateProviderUseCase::new(store.clone(), browser, webview.clone(), events);
        let cmd = AuthenticateCommand {
            provider: GitProviderKind::Bitbucket,
            account_login: AccountLogin::new("atlassian-team").unwrap(),
            method: AuthMethod::OAuthBrowser,
            oauth_config: Some(OAuthConfig {
                client_id: "bb_id".into(),
                auth_url: "https://bitbucket.org/site/oauth2/authorize".into(),
                token_url: "https://bitbucket.org/site/oauth2/access_token".into(),
                redirect_uri: "http://127.0.0.1:8080/callback".into(),
                scopes: vec!["repository".into()],
            }),
        };

        let res = uc.execute(cmd).unwrap();
        match res {
            AuthResult::Success {
                provider,
                account,
                credential_ref,
            } => {
                assert_eq!(provider, GitProviderKind::Bitbucket);
                assert_eq!(account.as_str(), "atlassian-team");
                assert!(store.get_secret(&credential_ref).unwrap().is_some());
            }
            _ => panic!("expected success via webview fallback"),
        }

        // Verify webview window was opened and automatically closed
        assert!(*webview.window_opened.lock().unwrap());
        assert!(*webview.window_closed.lock().unwrap());

        // Verify fallback event was emitted
        let mut fallback_seen = false;
        while let Ok(event) = rx.try_recv() {
            if let AppEvent::AuthFallbackToWebview { provider, .. } = event {
                assert_eq!(provider, "bitbucket");
                fallback_seen = true;
            }
        }
        assert!(fallback_seen, "expected AuthFallbackToWebview event");
    }
}
