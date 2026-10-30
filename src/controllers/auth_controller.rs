//! Authentication controller split by credential lifecycle and sign-in method.

//! Authentication and credential coordination controller.

use crate::app::{ReleaseDesk, find_webview_binary};
use crate::workspace::{AuthModeChoice, ProviderChoice};
use gpui_kit::{Context, Window};
use release_desk_core::port::SecretStorePort as _;
use release_desk_store::auth::{DevicePollOutcome, GitHubDeviceFlow, copy_to_macos_clipboard};
use release_desk_store::credentials::OsSecretStore;
use std::sync::Arc;

mod browser;
mod credentials;
mod token;
mod webview;
