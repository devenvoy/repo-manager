//! Project authentication browser coordination.

use super::*;

impl ReleaseDesk {
    pub(crate) fn trigger_browser_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.setup_provider.browser_auth() {
            BrowserAuth::GitLabPkce | BrowserAuth::AzureEntra | BrowserAuth::ForgePkce => {
                self.trigger_pkce_auth(window, cx);
                return;
            }
            BrowserAuth::Unavailable => {
                self.auth_status_message = Some(
                    "Native browser sign-in is not available for this provider. Use its access token or a detected Git account.".into(),
                );
                cx.notify();
                return;
            }
            BrowserAuth::GitHubDevice => {}
        }
        if self.setup_provider != ProviderChoice::GitHub {
            self.auth_status_message = Some(
                "Browser sign-in is not available for this provider. Use a validated API token."
                    .into(),
            );
            cx.notify();
            return;
        }

        let Some(client_id) = release_desk_store::auth::providers::github_client_id() else {
            self.auth_status_message =
                Some("Release Desk's GitHub OAuth client is not configured in this build.".into());
            cx.notify();
            return;
        };
        let client_id = client_id.to_string();

        if let Some(c) = self.device_flow_canceller.take() {
            c.store(true, std::sync::atomic::Ordering::SeqCst);
        }

        if self.setup_provider == ProviderChoice::GitHub {
            let flow = GitHubDeviceFlow::default();
            let scopes = ["repo", "read:user"];
            match flow.request_code(&client_id, &scopes) {
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
                        let cid = client_id.clone();
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
                }
                Err(e) => {
                    self.auth_status_message = Some(format!("Could not start GitHub OAuth ({e})."));
                    cx.notify();
                }
            }
        }
    }

    fn trigger_pkce_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let provider = self.setup_provider;
        let endpoint = self.custom_host_input.read(cx).value().trim().to_string();
        let config =
            match release_desk_store::auth::providers::oauth_config(provider.as_str(), &endpoint) {
                Ok(config) => config,
                Err(error) => {
                    self.auth_status_message = Some(error);
                    cx.notify();
                    return;
                }
            };
        if let Some(cancelled) = self.device_flow_canceller.take() {
            cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let authorization = match release_desk_store::auth::OAuthAuthorizationFlow::start(
            config,
            cancel_token.clone(),
        ) {
            Ok(authorization) => authorization,
            Err(error) => {
                self.auth_status_message = Some(error);
                cx.notify();
                return;
            }
        };
        let authorization_url = authorization.authorization_url().to_string();
        if let Err(error) =
            release_desk_store::auth::SystemBrowserAuthPort::new().open_url(&authorization_url)
        {
            self.auth_status_message = Some(format!("Could not open the sign-in browser: {error}"));
            cx.notify();
            return;
        }

        let secret_store = Arc::clone(&self.secret_store);
        self.device_flow_canceller = Some(cancel_token.clone());
        let provider_name = provider.as_str().to_string();
        let endpoint_for_validation = if provider == ProviderChoice::AzureDevOps {
            Some(endpoint.clone())
        } else {
            release_desk_store::auth::providers::forge_origin(provider.as_str(), &endpoint)
        };
        let endpoint_for_validation = endpoint_for_validation.map(|value| value.to_string());
        let endpoint_for_key = provider.requires_endpoint().then(|| endpoint.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_status_message = Some(format!(
            "Complete {} sign-in in your browser. Waiting for the secure callback…",
            provider.label()
        ));
        std::thread::spawn(move || {
            let result = (|| {
                let credential = authorization.wait_and_exchange()?;
                let token = credential.access_token();
                let login = release_desk_store::provider_sources::connection::validate_token(
                    &provider_name,
                    token,
                    endpoint_for_validation.as_deref(),
                )
                .map_err(|error| error.to_string())?;
                let credential_ref =
                    release_desk_store::provider_sources::connection::oauth_credential_key(
                        &provider_name,
                        &login,
                        endpoint_for_key.as_deref(),
                    );
                secret_store
                    .set_secret(&credential_ref, &credential.encode()?)
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>((login, credential_ref))
            })();
            let _ = tx.send(result);
        });

        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.device_flow_canceller = None;
                            if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
                                this.auth_status_message = None;
                                cx.notify();
                                return;
                            }
                            if !this.setup_active || this.setup_provider != provider {
                                return;
                            }
                            match result {
                                Ok((login, credential_ref)) => {
                                    this.saved_credential_ref = Some(credential_ref);
                                    this.selected_saved_account = Some(login.clone());
                                    this.auth_mode_choice = AuthModeChoice::SavedAccount;
                                    this.account_input.update(cx, |input, cx| {
                                        input.set_value(login.clone(), window, cx)
                                    });
                                    this.auth_status_message = Some(format!(
                                        "Connected to {} as @{login}.",
                                        provider.label()
                                    ));
                                }
                                Err(error) => {
                                    this.auth_status_message = Some(format!(
                                        "{} sign-in could not complete: {error}",
                                        provider.label()
                                    ));
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
        cx.notify();
    }

    pub(crate) fn cancel_browser_auth(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.device_flow_canceller.take() {
            c.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(mut child) = self.active_webview_child.take() {
            let _ = child.kill();
        }
        self.device_flow_user_code = None;
        self.auth_status_message = None;
        cx.notify();
    }
}
