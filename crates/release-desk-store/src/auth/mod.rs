//! Authentication adapters for browser launch and custom webview windows.

pub mod authorization_code;
pub mod browser_adapter;
pub mod device_flow;
pub mod local_server;
pub mod providers;
pub mod webview_adapter;

pub use authorization_code::{OAuthAuthorizationFlow, OAuthProviderConfig, StoredOAuthCredential};
pub use browser_adapter::SystemBrowserAuthPort;
pub use device_flow::{
    DeviceCodeResponse, DevicePollOutcome, GitHubDeviceFlow, copy_to_macos_clipboard,
};
pub use local_server::{LocalOAuthServer, OAuthRedirectOutcome};
pub use webview_adapter::ModalWebviewAuthPort;
