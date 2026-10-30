//! Project authentication webview coordination.

use super::*;

impl ReleaseDesk {
    pub(crate) fn trigger_webview_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.setup_provider != ProviderChoice::GitHub {
            self.auth_status_message = Some(
                "Webview sign-in is not available for this provider. Use a validated API token."
                    .into(),
            );
            cx.notify();
            return;
        }
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
                }
                Err(e) => {
                    self.auth_status_message = Some(format!("Could not start GitHub OAuth ({e})."));
                    cx.notify();
                }
            }
        }
    }
}
