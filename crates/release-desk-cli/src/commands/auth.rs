//! Authentication command handlers for multi-host Git providers.

use crate::args::AuthSubcommand;
use crate::context::CliContext;
use crate::output::{output_value, print_info};
use release_desk_application::use_case::auth::{AuthResult, AuthenticateCommand, OAuthConfig};
use release_desk_core::error::DomainError;
use release_desk_core::project::AccountLogin;
use release_desk_core::provider::{AuthMethod, GitProviderKind};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct AuthStatusReport {
    pub providers: HashMap<String, bool>,
}

#[derive(Serialize)]
pub struct LoginReport {
    pub provider: String,
    pub success: bool,
    pub account: String,
    pub message: String,
}

pub fn handle_auth(
    ctx: &CliContext,
    subcommand: AuthSubcommand,
    json: bool,
) -> Result<(), DomainError> {
    match subcommand {
        AuthSubcommand::Login {
            provider,
            token,
            browser,
        } => {
            let provider_kind = match provider.to_lowercase().as_str() {
                "gitlab" => GitProviderKind::GitLab,
                "bitbucket" | "atlassian" => GitProviderKind::Bitbucket,
                _ => GitProviderKind::GitHub,
            };

            let default_login = AccountLogin::new("default-user").map_err(DomainError::from)?;

            let (method, oauth_config) = if let Some(t) = token {
                (AuthMethod::Token { secret: t }, None)
            } else if browser {
                let oauth = OAuthConfig {
                    client_id: "release-desk-app".into(),
                    auth_url: format!(
                        "https://{}.com/login/oauth/authorize",
                        provider.to_lowercase()
                    ),
                    token_url: format!(
                        "https://{}.com/login/oauth/access_token",
                        provider.to_lowercase()
                    ),
                    redirect_uri: "http://127.0.0.1:4567/callback".into(),
                    scopes: vec!["repo".into(), "read:user".into()],
                };
                (AuthMethod::OAuthBrowser, Some(oauth))
            } else {
                return Err(DomainError::PortFailure {
                    port: "Auth",
                    message: "Please specify either --token <TOKEN> or --browser".into(),
                });
            };

            let cmd = AuthenticateCommand {
                provider: provider_kind,
                account_login: default_login,
                method,
                oauth_config,
            };

            let res = ctx.auth_uc.execute(cmd)?;

            match res {
                AuthResult::Success {
                    provider: p,
                    account: acc,
                    ..
                } => {
                    let report = LoginReport {
                        provider: format!("{:?}", p),
                        success: true,
                        account: acc.as_str().to_string(),
                        message: format!("Successfully authenticated with {:?}", p),
                    };

                    output_value(json, &report, |r| {
                        print_info(false, format!("✓ {} as '{}'", r.message, r.account));
                    });

                    Ok(())
                }
                AuthResult::Failed { provider: p, error } => Err(DomainError::PortFailure {
                    port: "Auth",
                    message: format!("Authentication failed for {:?}: {}", p, error),
                }),
                AuthResult::Cancelled => Err(DomainError::PortFailure {
                    port: "Auth",
                    message: "Authentication was cancelled by user".into(),
                }),
            }
        }
        AuthSubcommand::Status => {
            let providers = vec![
                GitProviderKind::GitHub,
                GitProviderKind::GitLab,
                GitProviderKind::Bitbucket,
            ];

            let mut map = HashMap::new();
            for p in providers {
                let key = format!("release_desk_{}_default-user", p.as_str());
                let fallback_key = format!("release_desk_{}", p.as_str());
                let is_authed = ctx.secret_store.get_secret(&key)?.is_some()
                    || ctx.secret_store.get_secret(&fallback_key)?.is_some();
                map.insert(format!("{:?}", p), is_authed);
            }

            let report = AuthStatusReport { providers: map };

            output_value(json, &report, |r| {
                print_info(false, "Git Provider Authentication Status:");
                for (prov, authed) in &r.providers {
                    let mark = if *authed {
                        "✓ Connected"
                    } else {
                        "✗ Not configured"
                    };
                    print_info(false, format!("  • {:<12} {}", prov, mark));
                }
            });

            Ok(())
        }
    }
}
