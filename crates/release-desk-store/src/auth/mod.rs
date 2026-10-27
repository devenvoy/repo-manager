//! Authentication adapters for browser launch and custom webview windows.

pub mod browser_adapter;
pub mod device_flow;
pub mod local_server;
pub mod webview_adapter;

pub use browser_adapter::SystemBrowserAuthPort;
pub use device_flow::{
    DEFAULT_GITHUB_CLIENT_ID, DeviceCodeResponse, DevicePollOutcome, GitHubDeviceFlow,
    copy_to_macos_clipboard,
};
pub use local_server::{LocalOAuthServer, OAuthRedirectOutcome};
pub use webview_adapter::ModalWebviewAuthPort;
