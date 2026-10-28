//! Integration tests for multi-host Git providers and browser/webview authentication.

use release_desk_application::event::{AppEvent, EventBroadcaster};
use release_desk_application::provider_registry::ProviderRegistry;
use release_desk_application::testing::FakeSecretStore;
use release_desk_application::use_case::auth::{
    AuthResult, AuthenticateCommand, AuthenticateProviderUseCase, OAuthConfig,
};
use release_desk_core::port::SecretStorePort;
use release_desk_core::project::AccountLogin;
use release_desk_core::provider::{AuthCallbackStatus, AuthMethod, GitProviderKind};
use release_desk_store::auth::{ModalWebviewAuthPort, SystemBrowserAuthPort};
use release_desk_store::bitbucket_client::HttpBitbucketGateway;
use release_desk_store::github_client::HttpGitHubGateway;
use release_desk_store::gitlab_client::HttpGitLabGateway;
use std::sync::Arc;

#[test]
fn multi_host_provider_registry_integration() {
    let registry = ProviderRegistry::new();

    // 1. Connect GitHub, GitLab, and Atlassian Bitbucket
    let gh = Arc::new(HttpGitHubGateway::new(Some("gh_token".into())));
    let gl = Arc::new(HttpGitLabGateway::new(Some("gl_token".into())));
    let bb = Arc::new(HttpBitbucketGateway::new(Some("bb_token".into())));

    registry.register(gh);
    registry.register(gl);
    registry.register(bb);

    let providers = registry.supported_providers();
    assert_eq!(
        providers,
        vec![
            GitProviderKind::GitHub,
            GitProviderKind::GitLab,
            GitProviderKind::Bitbucket,
        ]
    );

    assert!(registry.get(&GitProviderKind::GitHub).is_ok());
    assert!(registry.get(&GitProviderKind::GitLab).is_ok());
    assert!(registry.get(&GitProviderKind::Bitbucket).is_ok());
}

#[test]
fn token_authentication_for_multiple_hosts() {
    let secret_store = Arc::new(FakeSecretStore::new());
    let browser = Arc::new(SystemBrowserAuthPort::new());
    let webview = Arc::new(ModalWebviewAuthPort::new());
    let events = EventBroadcaster::new(32);

    let auth_uc = AuthenticateProviderUseCase::new(secret_store.clone(), browser, webview, events);

    // Auth with GitHub token
    let gh_res = auth_uc
        .execute(AuthenticateCommand {
            provider: GitProviderKind::GitHub,
            account_login: AccountLogin::new("octocat").unwrap(),
            method: AuthMethod::Token {
                secret: "ghp_mock_token_abc".into(),
            },
            oauth_config: None,
        })
        .unwrap();

    assert!(matches!(gh_res, AuthResult::Success { .. }));
    assert_eq!(
        secret_store
            .get_secret("release_desk_github_octocat")
            .unwrap()
            .as_deref(),
        Some("ghp_mock_token_abc")
    );

    // Auth with GitLab token
    let gl_res = auth_uc
        .execute(AuthenticateCommand {
            provider: GitProviderKind::GitLab,
            account_login: AccountLogin::new("tanuki").unwrap(),
            method: AuthMethod::Token {
                secret: "glpat_mock_token_xyz".into(),
            },
            oauth_config: None,
        })
        .unwrap();

    assert!(matches!(gl_res, AuthResult::Success { .. }));
    assert_eq!(
        secret_store
            .get_secret("release_desk_gitlab_tanuki")
            .unwrap()
            .as_deref(),
        Some("glpat_mock_token_xyz")
    );

    // Auth with Atlassian Bitbucket token
    let bb_res = auth_uc
        .execute(AuthenticateCommand {
            provider: GitProviderKind::Bitbucket,
            account_login: AccountLogin::new("atlassian-dev").unwrap(),
            method: AuthMethod::Token {
                secret: "bb_app_password_123".into(),
            },
            oauth_config: None,
        })
        .unwrap();

    assert!(matches!(bb_res, AuthResult::Success { .. }));
    assert_eq!(
        secret_store
            .get_secret("release_desk_bitbucket_atlassian-dev")
            .unwrap()
            .as_deref(),
        Some("bb_app_password_123")
    );
}

#[test]
fn browser_auth_with_automatic_webview_fallback_when_browser_missing() {
    let secret_store = Arc::new(FakeSecretStore::new());
    // Simulate system browser missing / not found
    let browser = Arc::new(SystemBrowserAuthPort::new_simulating_missing());
    let webview = Arc::new(ModalWebviewAuthPort::with_simulated_outcome(
        AuthCallbackStatus::Success {
            code: "oauth_code_secure_999".into(),
        },
    ));
    let events = EventBroadcaster::new(32);
    let mut rx = events.subscribe();

    let auth_uc =
        AuthenticateProviderUseCase::new(secret_store.clone(), browser, webview.clone(), events);

    let res = auth_uc
        .execute(AuthenticateCommand {
            provider: GitProviderKind::GitHub,
            account_login: AccountLogin::new("dev-user").unwrap(),
            method: AuthMethod::OAuthBrowser,
            oauth_config: Some(OAuthConfig {
                client_id: "client_123".into(),
                auth_url: "https://github.com/login/oauth/authorize".into(),
                token_url: "https://github.com/login/oauth/access_token".into(),
                redirect_uri: "http://127.0.0.1:8080/callback".into(),
                scopes: vec!["repo".into(), "read:user".into()],
            }),
        })
        .unwrap();

    assert!(matches!(res, AuthResult::Failed { error, .. } if error.contains("exchange")));
    assert!(webview.was_window_closed());
    assert!(
        secret_store
            .get_secret("release_desk_github_dev-user")
            .unwrap()
            .is_none()
    );

    // Verify fallback event was emitted
    let mut saw_fallback = false;
    let mut saw_failure = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::AuthFallbackToWebview { provider, reason } => {
                assert_eq!(provider, "github");
                assert!(reason.contains("browser unavailable") || reason.contains("browser"));
                saw_fallback = true;
            }
            AppEvent::AuthFailed { provider, error } => {
                assert_eq!(provider, "github");
                assert!(error.contains("exchange"));
                saw_failure = true;
            }
            _ => {}
        }
    }
    assert!(saw_fallback, "expected AuthFallbackToWebview event");
    assert!(saw_failure, "expected AuthFailed event");
}

#[test]
fn webview_window_closes_on_auth_failure() {
    let secret_store = Arc::new(FakeSecretStore::new());
    let browser = Arc::new(SystemBrowserAuthPort::new_simulating_missing());
    // Simulate user access denied in webview
    let webview = Arc::new(ModalWebviewAuthPort::with_simulated_outcome(
        AuthCallbackStatus::Failure {
            error: "access_denied".into(),
        },
    ));
    let events = EventBroadcaster::new(32);

    let auth_uc = AuthenticateProviderUseCase::new(secret_store, browser, webview.clone(), events);

    let res = auth_uc
        .execute(AuthenticateCommand {
            provider: GitProviderKind::GitLab,
            account_login: AccountLogin::new("denied-user").unwrap(),
            method: AuthMethod::OAuthWebview,
            oauth_config: Some(OAuthConfig {
                client_id: "gitlab_id".into(),
                auth_url: "https://gitlab.com/oauth/authorize".into(),
                token_url: "https://gitlab.com/oauth/token".into(),
                redirect_uri: "http://127.0.0.1:8080/callback".into(),
                scopes: vec!["api".into()],
            }),
        })
        .unwrap();

    assert_eq!(
        res,
        AuthResult::Failed {
            provider: GitProviderKind::GitLab,
            error: "access_denied".into(),
        }
    );
    // Verify window was automatically closed on failure
    assert!(webview.was_window_closed());
}
