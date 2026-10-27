//! System default browser launcher adapter.
//!
//! Implements `BrowserAuthPort` using system commands with detection
//! of missing browsers to trigger automatic webview panel fallback.

use release_desk_core::error::DomainError;
use release_desk_core::port::BrowserAuthPort;
use std::process::Command;

/// System default browser launcher.
#[derive(Clone, Debug, Default)]
pub struct SystemBrowserAuthPort {
    simulate_browser_missing: bool,
}

impl SystemBrowserAuthPort {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an instance simulating that no system browser is installed
    /// (for verifying the automated webview fallback flow).
    pub fn new_simulating_missing() -> Self {
        Self {
            simulate_browser_missing: true,
        }
    }
}

impl BrowserAuthPort for SystemBrowserAuthPort {
    fn open_url(&self, url: &str) -> Result<(), DomainError> {
        if self.simulate_browser_missing {
            return Err(DomainError::PortFailure {
                port: "Browser",
                message: "No default browser installed on this system".into(),
            });
        }

        #[cfg(target_os = "macos")]
        let mut cmd = Command::new("open");

        #[cfg(target_os = "linux")]
        let mut cmd = Command::new("xdg-open");

        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/C", "start", ""]);
            c
        };

        cmd.arg(url);

        match cmd.spawn() {
            Ok(_) => Ok(()),
            Err(e) => Err(DomainError::PortFailure {
                port: "Browser",
                message: format!("failed to launch system browser: {e}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulate_missing_browser_returns_error() {
        let port = SystemBrowserAuthPort::new_simulating_missing();
        let err = port.open_url("https://github.com/login").unwrap_err();
        match err {
            DomainError::PortFailure { port, message } => {
                assert_eq!(port, "Browser");
                assert!(message.contains("No default browser"));
            }
            other => panic!("expected PortFailure, got {other:?}"),
        }
    }
}
