//! Authentication controller split by credential lifecycle and sign-in method.

//! Authentication and credential coordination controller.

use crate::app::{ReleaseDesk, find_webview_binary};
use crate::workspace::{AuthModeChoice, BrowserAuth, ProviderChoice};
use gpui_kit::{Context, Window};
use release_desk_core::port::{BrowserAuthPort as _, SecretStorePort as _};
use release_desk_store::auth::{DevicePollOutcome, GitHubDeviceFlow, copy_to_macos_clipboard};
use release_desk_store::credentials::OsSecretStore;
use std::sync::Arc;

mod browser;
mod credentials;
mod token;
mod webview;

pub(crate) fn browser_auth_available(provider: ProviderChoice) -> bool {
    match provider.browser_auth() {
        BrowserAuth::Unavailable => false,
        BrowserAuth::GitHubDevice
        | BrowserAuth::GitLabPkce
        | BrowserAuth::AzureEntra
        | BrowserAuth::ForgePkce => {
            release_desk_store::auth::providers::has_configured_client(provider.as_str())
        }
    }
}
