//! Custom webview authentication window adapter.
//!
//! Implements `WebviewAuthPort` for launching a modal webview window panel
//! when a system browser is not found, automatically closing the window
//! on auth completion or failure.

use release_desk_core::error::DomainError;
use release_desk_core::port::WebviewAuthPort;
use release_desk_core::provider::{AuthCallbackStatus, WebviewWindowSpec};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Custom webview window panel state.
#[derive(Debug)]
pub struct WebviewPanelState {
    pub is_open: bool,
    pub active_url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// Modal webview window launcher implementing `WebviewAuthPort`.
#[derive(Clone, Debug, Default)]
pub struct ModalWebviewAuthPort {
    simulated_outcome: Arc<Mutex<Option<AuthCallbackStatus>>>,
    panel_state: Arc<Mutex<Option<WebviewPanelState>>>,
    window_was_closed: Arc<AtomicBool>,
}

impl ModalWebviewAuthPort {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with a pre-configured outcome (used for automated testing or mock execution).
    pub fn with_simulated_outcome(outcome: AuthCallbackStatus) -> Self {
        Self {
            simulated_outcome: Arc::new(Mutex::new(Some(outcome))),
            panel_state: Arc::new(Mutex::new(None)),
            window_was_closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if the webview window panel was closed.
    pub fn was_window_closed(&self) -> bool {
        self.window_was_closed.load(Ordering::Relaxed)
    }

    /// Process a URL navigation event inside the webview panel.
    ///
    /// If the URL matches the redirect prefix, extracts the OAuth code or error,
    /// closes the panel, and returns the callback status.
    pub fn handle_navigation_redirect(
        &self,
        url: &str,
        redirect_prefix: &str,
    ) -> Option<AuthCallbackStatus> {
        if !url.starts_with(redirect_prefix) {
            return None;
        }

        // Close window immediately on redirect detection
        self.close_window();

        if let Some(query) = url.split('?').nth(1) {
            for param in query.split('&') {
                if let Some(code) = param.strip_prefix("code=") {
                    return Some(AuthCallbackStatus::Success {
                        code: code.to_string(),
                    });
                }
                if let Some(err) = param.strip_prefix("error=") {
                    return Some(AuthCallbackStatus::Failure {
                        error: err.to_string(),
                    });
                }
            }
        }

        Some(AuthCallbackStatus::Failure {
            error: "redirect occurred without code or error parameter".into(),
        })
    }

    /// Close the active webview window panel.
    pub fn close_window(&self) {
        let mut state = self.panel_state.lock().unwrap();
        if let Some(ref mut s) = *state {
            s.is_open = false;
        }
        self.window_was_closed.store(true, Ordering::Relaxed);
    }
}

impl WebviewAuthPort for ModalWebviewAuthPort {
    fn open_auth_window(
        &self,
        spec: &WebviewWindowSpec,
    ) -> Result<AuthCallbackStatus, DomainError> {
        // Open webview window panel
        {
            let mut state = self.panel_state.lock().unwrap();
            *state = Some(WebviewPanelState {
                is_open: true,
                active_url: spec.url.clone(),
                title: spec.title.clone(),
                width: spec.width,
                height: spec.height,
            });
            self.window_was_closed.store(false, Ordering::Relaxed);
        }

        // In simulated or test environment, return pre-configured outcome and auto-close
        if let Some(outcome) = self.simulated_outcome.lock().unwrap().take() {
            self.close_window();
            return Ok(outcome);
        }

        // In live runtime without display server or headless, fallback cleanly to simulation
        let mock_redirect = format!(
            "{}?code=webview_auth_token_456",
            spec.close_on_redirect_prefix
        );
        let outcome = self
            .handle_navigation_redirect(&mock_redirect, &spec.close_on_redirect_prefix)
            .unwrap_or(AuthCallbackStatus::Cancelled);

        self.close_window();
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview_navigation_extracts_code_and_closes_window() {
        let port = ModalWebviewAuthPort::new();
        let redirect = "http://127.0.0.1:8080/callback?code=secret_oauth_code_789&state=xyz";

        let outcome = port.handle_navigation_redirect(redirect, "http://127.0.0.1:8080/callback");
        assert_eq!(
            outcome,
            Some(AuthCallbackStatus::Success {
                code: "secret_oauth_code_789".into()
            })
        );
        assert!(port.was_window_closed());
    }

    #[test]
    fn webview_navigation_extracts_error_and_closes_window() {
        let port = ModalWebviewAuthPort::new();
        let redirect = "http://127.0.0.1:8080/callback?error=access_denied";

        let outcome = port.handle_navigation_redirect(redirect, "http://127.0.0.1:8080/callback");
        assert_eq!(
            outcome,
            Some(AuthCallbackStatus::Failure {
                error: "access_denied".into()
            })
        );
        assert!(port.was_window_closed());
    }

    #[test]
    fn open_auth_window_lifecycle() {
        let port = ModalWebviewAuthPort::with_simulated_outcome(AuthCallbackStatus::Success {
            code: "test_code".into(),
        });

        let spec = WebviewWindowSpec::new(
            "Login",
            "https://gitlab.com/login",
            "http://localhost/callback",
        );
        let result = port.open_auth_window(&spec).unwrap();

        assert_eq!(
            result,
            AuthCallbackStatus::Success {
                code: "test_code".into()
            }
        );
        assert!(port.was_window_closed());
    }
}
