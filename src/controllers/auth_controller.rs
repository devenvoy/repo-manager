//! Authentication and credential coordination controller.

use crate::app::{ReleaseDesk, find_webview_binary};
use crate::workspace::{AuthModeChoice, ProviderChoice};
use gpui_kit::{Context, Window};
use release_desk_core::port::SecretStorePort as _;
use release_desk_store::auth::{
    DevicePollOutcome, GitHubDeviceFlow, LocalOAuthServer, OAuthRedirectOutcome,
    copy_to_macos_clipboard,
};
use std::sync::Arc;

impl ReleaseDesk {
    pub(crate) fn connect_system_credential(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((ref username, ref token)) = self.detected_system_credential {
            let cred_key = format!("release_desk_{}_{}", self.setup_provider.as_str(), username);
            if let Err(error) = self.secret_store.set_secret(&cred_key, token) {
                self.message = format!("Could not save account credential: {error}");
                cx.notify();
                return;
            }
            self.saved_credential_ref = Some(cred_key);
            self.selected_saved_account = Some(username.clone());
            self.auth_mode_choice = AuthModeChoice::SavedAccount;
            self.auth_status_message = Some(format!(
                "Connected with {} API as @{} (5,000 req/hr). Token stored in Keychain.",
                self.setup_provider.label(),
                username
            ));
            self.account_input.update(cx, |input, cx| {
                input.set_value(username.clone(), window, cx)
            });
            self.message = format!("Connected with system Git credentials for @{username}.");
        }
        cx.notify();
    }

    pub(crate) fn select_saved_account(
        &mut self,
        handle: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cred_key = format!("release_desk_{}_{}", self.setup_provider.as_str(), handle);
        let secret_store = self.secret_store.clone();
        let provider = self.setup_provider;
        let (tx, rx) = std::sync::mpsc::channel();
        self.message = "Checking saved account…".into();
        cx.notify();
        let key_for_lookup = cred_key.clone();
        std::thread::spawn(move || {
            let _ = tx.send(secret_store.get_secret(&key_for_lookup));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            if !this.setup_active || this.setup_provider != provider {
                                return;
                            }
                            match result {
                                Ok(Some(_)) => {
                                    this.selected_saved_account = Some(handle.clone());
                                    this.saved_credential_ref = Some(cred_key);
                                    this.auth_mode_choice = AuthModeChoice::SavedAccount;
                                    this.auth_status_message = Some(format!(
                                        "Using Keychain credentials for @{handle}. No token needed!"
                                    ));
                                    this.account_input.update(cx, |input, cx| {
                                        input.set_value(handle, window, cx)
                                    });
                                    this.message.clear();
                                }
                                Ok(None) => {
                                    this.message =
                                        "Saved account credential is missing. Connect again.".into()
                                }
                                Err(error) => {
                                    this.message = format!("Cannot read saved account: {error}")
                                }
                            }
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }

    pub(crate) fn disconnect_credential(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref mut server) = self.local_oauth_server {
            server.stop();
        }
        self.local_oauth_server = None;
        if let Some(mut child) = self.active_webview_child.take() {
            let _ = child.kill();
        }
        if let Some(c) = self.device_flow_canceller.take() {
            c.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.selected_saved_account = None;
        self.saved_credential_ref = None;
        self.auth_status_message = None;
        self.device_flow_user_code = None;
        self.account_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.token_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.message = "Disconnected account credentials.".into();
        cx.notify();
    }

    pub(crate) fn select_auth_tab(&mut self, mode: AuthModeChoice, cx: &mut Context<Self>) {
        self.auth_mode_choice = mode;
        cx.notify();
    }

    pub(crate) fn copy_user_code(&mut self, code: String, cx: &mut Context<Self>) {
        copy_to_macos_clipboard(&code);
        self.message = format!("Code {code} copied to clipboard.");
        cx.notify();
    }

    pub(crate) fn trigger_browser_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref mut server) = self.local_oauth_server {
            server.stop();
        }
        self.local_oauth_server = None;

        if let Some(c) = self.device_flow_canceller.take() {
            c.store(true, std::sync::atomic::Ordering::SeqCst);
        }

        let custom_client_id = self
            .oauth_client_id_input
            .read(cx)
            .value()
            .trim()
            .to_string();

        if self.setup_provider == ProviderChoice::GitHub {
            let flow = GitHubDeviceFlow::default();
            let scopes = ["repo", "read:user"];
            match flow.request_code(&custom_client_id, &scopes) {
                Ok(resp) => {
                    let user_code = resp.user_code;
                    let device_code = resp.device_code;
                    let verify_url = resp.verification_uri;
                    let interval = resp.interval.max(5);

                    copy_to_macos_clipboard(&user_code);

                    self.device_flow_user_code = Some(user_code.clone());
                    self.device_flow_verification_uri = Some(verify_url.clone());
                    self.auth_mode_choice = AuthModeChoice::OAuthBrowser;
                    self.auth_status_message = Some(format!(
                        "Enter code {user_code} on GitHub (copied to clipboard). Waiting for authorization..."
                    ));
                    self.message = format!("One-time code {user_code} copied to clipboard.");

                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&verify_url).spawn();
                    #[cfg(not(target_os = "macos"))]
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&verify_url)
                        .spawn();

                    let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    self.device_flow_canceller = Some(cancel_token.clone());

                    cx.spawn_in(window, async move |this, cx| {
                        let cid = custom_client_id.clone();
                        let dcode = device_code.clone();
                        for _ in 0..180 {
                            cx.background_executor()
                                .timer(std::time::Duration::from_secs(interval))
                                .await;

                            if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
                                break;
                            }

                            let flow_bg = GitHubDeviceFlow::default();
                            let cid_clone = cid.clone();
                            let dcode_clone = dcode.clone();
                            let outcome = cx
                                .background_executor()
                                .spawn(async move {
                                    flow_bg.poll_token(&cid_clone, &dcode_clone)
                                })
                                .await;

                            match outcome {
                                Ok(DevicePollOutcome::Success { access_token, .. }) => {
                                    let flow_bg2 = GitHubDeviceFlow::default();
                                    let token_clone = access_token.clone();
                                    let user_login = cx
                                        .background_executor()
                                        .spawn(async move {
                                             flow_bg2.fetch_user_login(&token_clone)
                                        })
                                        .await
                                        .unwrap_or_else(|_| "github-user".to_string());

                                    let _ = this.update_in(cx, |this, window, cx| {
                                        let cred_key = format!("release_desk_github_{user_login}");
                                        if let Err(error) = this.secret_store.set_secret(&cred_key, &access_token) {
                                            this.message = format!("Could not save account credential: {error}");
                                            cx.notify();
                                            return;
                                        }
                                        this.saved_credential_ref = Some(cred_key);
                                        this.selected_saved_account = Some(user_login.clone());
                                        this.auth_status_message = Some(format!(
                                            "Successfully authorized via GitHub OAuth (@{user_login})."
                                        ));
                                        this.message = format!("Connected to GitHub as @{user_login}.");
                                        this.account_input.update(cx, |input, cx| {
                                            input.set_value(user_login, window, cx);
                                        });
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                                Ok(DevicePollOutcome::Pending) => {}
                                Ok(DevicePollOutcome::SlowDown) => {
                                    cx.background_executor()
                                        .timer(std::time::Duration::from_secs(5))
                                        .await;
                                }
                                Ok(DevicePollOutcome::Expired) => {
                                    let _ = this.update_in(cx, |this, _window, cx| {
                                        this.auth_status_message = Some(
                                            "Device code expired. Please click Start Sign-In again."
                                                .into(),
                                        );
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                                Ok(DevicePollOutcome::AccessDenied) => {
                                    let _ = this.update_in(cx, |this, _window, cx| {
                                        this.auth_status_message = Some(
                                            "Authorization was denied by user on GitHub.".into(),
                                        );
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                                Ok(DevicePollOutcome::Error(err)) => {
                                    let _ = this.update_in(cx, |this, _window, cx| {
                                        this.auth_status_message =
                                            Some(format!("OAuth error: {err}"));
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                                Err(e) => {
                                    let _ = this.update_in(cx, |this, _window, cx| {
                                        this.auth_status_message =
                                            Some(format!("Connection error: {e}"));
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                            }
                        }
                    })
                    .detach();

                    cx.notify();
                    return;
                }
                Err(e) => {
                    self.auth_status_message = Some(format!("Could not start GitHub OAuth ({e})."));
                    cx.notify();
                    return;
                }
            }
        }

        if !custom_client_id.is_empty() {
            match LocalOAuthServer::start(4567) {
                Ok(server) => {
                    let redirect_uri = format!("http://127.0.0.1:{}/callback", server.port());
                    let url = self
                        .setup_provider
                        .oauth_authorize_url(&custom_client_id, &redirect_uri);

                    self.local_oauth_server = Some(server);
                    self.auth_mode_choice = AuthModeChoice::OAuthBrowser;
                    self.auth_status_message = Some(
                        "Listening on http://127.0.0.1:4567/callback... Waiting for browser authorization.".into(),
                    );
                    self.message = "Launched browser OAuth. Listening on local port 4567.".into();

                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                    #[cfg(not(target_os = "macos"))]
                    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();

                    cx.spawn_in(window, async move |this, cx| {
                        for _ in 0..600 {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(100))
                                .await;
                            let outcome = this.update_in(cx, |this, _window, _cx| {
                                this.local_oauth_server
                                    .as_ref()
                                    .and_then(|s| s.poll_outcome())
                            });

                            if let Ok(Some(outcome)) = outcome {
                                let _ = this.update_in(cx, |this, _window, cx| {
                                    match outcome {
                                        OAuthRedirectOutcome::Success { .. } => {
                                            this.message = "Browser sign-in returned an authorization code, but token exchange is not available. Use a personal access token or device sign-in.".into();
                                            this.auth_status_message = Some(this.message.clone());
                                        }
                                        OAuthRedirectOutcome::Error { message } => {
                                            this.auth_status_message = Some(format!(
                                                "Browser authentication failed: {message}"
                                            ));
                                        }
                                    }
                                    if let Some(ref mut s) = this.local_oauth_server {
                                        s.stop();
                                    }
                                    this.local_oauth_server = None;
                                    cx.notify();
                                });
                                break;
                            }

                            let is_still_listening = this.update_in(cx, |this, _window, _cx| {
                                this.local_oauth_server
                                    .as_ref()
                                    .is_some_and(|s| s.is_listening())
                            });
                            if let Ok(false) = is_still_listening {
                                break;
                            }
                        }
                    })
                    .detach();

                    cx.notify();
                }
                Err(e) => {
                    self.auth_status_message = Some(format!(
                        "Could not start local redirect server on port 4567 ({e}). Please use Personal Access Token."
                    ));
                    cx.notify();
                }
            }
        } else {
            self.auth_status_message = Some(
                "Please configure an OAuth Client ID for this provider, or use Personal Access Token."
                    .into(),
            );
            cx.notify();
        }
    }

    pub(crate) fn cancel_browser_auth(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.device_flow_canceller.take() {
            c.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(mut child) = self.active_webview_child.take() {
            let _ = child.kill();
        }
        if let Some(ref mut server) = self.local_oauth_server {
            server.stop();
        }
        self.local_oauth_server = None;
        self.device_flow_user_code = None;
        self.auth_status_message = None;
        cx.notify();
    }

    pub(crate) fn trigger_webview_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = self.device_flow_canceller.take() {
            c.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(mut child) = self.active_webview_child.take() {
            let _ = child.kill();
        }

        let custom_client_id = self
            .oauth_client_id_input
            .read(cx)
            .value()
            .trim()
            .to_string();

        if self.setup_provider == ProviderChoice::GitHub {
            let flow = GitHubDeviceFlow::default();
            let scopes = ["repo", "read:user"];
            match flow.request_code(&custom_client_id, &scopes) {
                Ok(resp) => {
                    let user_code = resp.user_code;
                    let device_code = resp.device_code;
                    let verify_url = resp.verification_uri;
                    let interval = resp.interval.max(5);

                    copy_to_macos_clipboard(&user_code);

                    self.device_flow_user_code = Some(user_code.clone());
                    self.device_flow_verification_uri = Some(verify_url.clone());
                    self.auth_mode_choice = AuthModeChoice::OAuthWebview;
                    self.auth_status_message = Some(format!(
                        "Enter code {user_code} in the webview window (copied to clipboard)."
                    ));
                    self.message = format!("One-time code {user_code} copied to clipboard.");

                    if let Some(webview_bin) = find_webview_binary() {
                        if let Ok(child) = std::process::Command::new(webview_bin)
                            .arg(&verify_url)
                            .spawn()
                        {
                            self.active_webview_child = Some(child);
                        }
                    } else {
                        #[cfg(target_os = "macos")]
                        let _ = std::process::Command::new("open").arg(&verify_url).spawn();
                        #[cfg(not(target_os = "macos"))]
                        let _ = std::process::Command::new("xdg-open")
                            .arg(&verify_url)
                            .spawn();
                    }

                    let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    self.device_flow_canceller = Some(cancel_token.clone());

                    cx.spawn_in(window, async move |this, cx| {
                        let cid = custom_client_id.clone();
                        let dcode = device_code.clone();
                        for _ in 0..180 {
                            cx.background_executor()
                                .timer(std::time::Duration::from_secs(interval))
                                .await;

                            if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
                                break;
                            }

                            let flow_bg = GitHubDeviceFlow::default();
                            let cid_clone = cid.clone();
                            let dcode_clone = dcode.clone();
                            let outcome = cx
                                .background_executor()
                                .spawn(async move {
                                    flow_bg.poll_token(&cid_clone, &dcode_clone)
                                })
                                .await;

                            match outcome {
                                Ok(DevicePollOutcome::Success { access_token, .. }) => {
                                    let flow_bg2 = GitHubDeviceFlow::default();
                                    let token_clone = access_token.clone();
                                    let user_login = cx
                                        .background_executor()
                                        .spawn(async move {
                                            flow_bg2.fetch_user_login(&token_clone)
                                        })
                                        .await
                                        .unwrap_or_else(|_| "github-user".to_string());

                                    let _ = this.update_in(cx, |this, window, cx| {
                                        if let Some(mut child) = this.active_webview_child.take() {
                                            let _ = child.kill();
                                        }
                                        let cred_key = format!("release_desk_github_{user_login}");
                                        if let Err(error) = this.secret_store.set_secret(&cred_key, &access_token) {
                                            this.message = format!("Could not save account credential: {error}");
                                            cx.notify();
                                            return;
                                        }
                                        this.saved_credential_ref = Some(cred_key);
                                        this.selected_saved_account = Some(user_login.clone());
                                        this.auth_status_message = Some(format!(
                                            "Successfully authorized via In-App Webview OAuth (@{user_login})."
                                        ));
                                        this.message = format!("Connected to GitHub as @{user_login}.");
                                        this.account_input.update(cx, |input, cx| {
                                            input.set_value(user_login, window, cx);
                                        });
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                                Ok(DevicePollOutcome::Pending) => {}
                                Ok(DevicePollOutcome::SlowDown) => {
                                    cx.background_executor()
                                        .timer(std::time::Duration::from_secs(5))
                                        .await;
                                }
                                Ok(DevicePollOutcome::Expired) => {
                                    let _ = this.update_in(cx, |this, _window, cx| {
                                        if let Some(mut child) = this.active_webview_child.take() {
                                            let _ = child.kill();
                                        }
                                        this.auth_status_message = Some(
                                            "Device code expired. Please click Open Webview Sign-In again."
                                                .into(),
                                        );
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                                Ok(DevicePollOutcome::AccessDenied) => {
                                    let _ = this.update_in(cx, |this, _window, cx| {
                                        if let Some(mut child) = this.active_webview_child.take() {
                                            let _ = child.kill();
                                        }
                                        this.auth_status_message =
                                            Some("Authorization was denied by user.".into());
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                                Ok(DevicePollOutcome::Error(err)) => {
                                    let _ = this.update_in(cx, |this, _window, cx| {
                                        if let Some(mut child) = this.active_webview_child.take() {
                                            let _ = child.kill();
                                        }
                                        this.auth_status_message =
                                            Some(format!("OAuth error: {err}"));
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                                Err(e) => {
                                    let _ = this.update_in(cx, |this, _window, cx| {
                                        if let Some(mut child) = this.active_webview_child.take() {
                                            let _ = child.kill();
                                        }
                                        this.auth_status_message =
                                            Some(format!("Connection error: {e}"));
                                        this.device_flow_user_code = None;
                                        this.device_flow_canceller = None;
                                        cx.notify();
                                    });
                                    break;
                                }
                            }
                        }
                    })
                    .detach();

                    cx.notify();
                    return;
                }
                Err(e) => {
                    self.auth_status_message = Some(format!("Could not start GitHub OAuth ({e})."));
                    cx.notify();
                    return;
                }
            }
        }

        let target_url = if !custom_client_id.is_empty() {
            self.setup_provider
                .oauth_authorize_url(&custom_client_id, "http://127.0.0.1:4567/callback")
        } else {
            self.setup_provider.token_creation_url().to_string()
        };

        if let Some(webview_bin) = find_webview_binary() {
            let callback = if !custom_client_id.is_empty() {
                "http://127.0.0.1:4567/callback"
            } else {
                ""
            };
            if let Ok(child) = std::process::Command::new(webview_bin)
                .arg(&target_url)
                .arg(callback)
                .spawn()
            {
                self.active_webview_child = Some(child);
            }
            self.auth_mode_choice = AuthModeChoice::OAuthWebview;
            self.auth_status_message = Some(
                "In-App Webview opened in dedicated modal window. Complete authorization there."
                    .into(),
            );
            self.message = "Launched in-app WebKit modal window.".into();
        } else {
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&target_url).spawn();
            #[cfg(not(target_os = "macos"))]
            let _ = std::process::Command::new("xdg-open")
                .arg(&target_url)
                .spawn();
            self.auth_status_message = Some("Opened sign-in page in browser.".into());
        }
        cx.notify();
    }

    pub(crate) fn save_token_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.token_input.read(cx).value().trim().to_string();
        if token.is_empty() {
            self.message = "Please enter a valid personal access token.".into();
            cx.notify();
            return;
        }
        let cred_key = format!("release_desk_{}_pat", self.setup_provider.as_str());
        if let Err(error) = self.secret_store.set_secret(&cred_key, &token) {
            self.message = format!("Could not save token: {error}");
            cx.notify();
            return;
        }
        self.saved_credential_ref = Some(cred_key);
        self.selected_saved_account = Some("token-user".into());
        self.auth_status_message =
            Some("Personal Access Token saved securely in macOS Keychain (5,000 req/hr).".into());
        self.message = "Personal Access Token stored in Keychain.".into();
        self.account_input.update(cx, |input, cx| {
            input.set_value("token-user", window, cx);
        });
        cx.notify();
    }

    pub(crate) fn open_token_creation_url(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let url = self.setup_provider.token_creation_url();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(url).spawn();
        #[cfg(not(target_os = "macos"))]
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        self.auth_status_message = Some("Opened provider token creation page in browser.".into());
        cx.notify();
    }
}
