//! Authentication adapters for browser launch and custom webview windows.

pub mod browser_adapter;
pub mod local_server;
pub mod webview_adapter;

pub use browser_adapter::SystemBrowserAuthPort;
pub use local_server::{LocalOAuthServer, OAuthRedirectOutcome};
pub use webview_adapter::ModalWebviewAuthPort;
