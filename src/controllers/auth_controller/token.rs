//! Project authentication token coordination.

use super::*;

impl ReleaseDesk {
    pub(crate) fn save_token_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.token_input.read(cx).value().trim().to_string();
        if token.is_empty() {
            self.message = "Please enter a valid personal access token.".into();
            cx.notify();
            return;
        }
        let provider = self.setup_provider;
        let endpoint = self.custom_host_input.read(cx).value().trim().to_string();
        let secret_store = self.secret_store.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_status_message = Some(format!("Checking {} token…", provider.label()));
        std::thread::spawn(move || {
            let result = (|| {
                let login = release_desk_store::provider_sources::connection::validate_token(
                    provider.as_str(),
                    &token,
                    (!endpoint.is_empty()).then_some(endpoint.as_str()),
                )
                .map_err(|error| error.to_string())?;
                let key = release_desk_store::provider_sources::connection::credential_key(
                    provider.as_str(),
                    &login,
                    (!endpoint.is_empty()).then_some(endpoint.as_str()),
                );
                secret_store
                    .set_secret(&key, &token)
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>((login, key))
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
                            if !this.setup_active || this.setup_provider != provider {
                                return;
                            }
                            match result {
                                Ok((login, key)) => {
                                    this.saved_credential_ref = Some(key);
                                    this.selected_saved_account = Some(login.clone());
                                    this.auth_mode_choice = AuthModeChoice::SavedAccount;
                                    this.auth_status_message = Some(format!(
                                        "Connected to {} as @{login}.",
                                        provider.label()
                                    ));
                                    this.account_input
                                        .update(cx, |input, cx| input.set_value(login, window, cx));
                                    this.token_input.update(cx, |input, cx| {
                                        input.set_value(String::new(), window, cx)
                                    });
                                }
                                Err(error) => {
                                    this.auth_status_message = Some(format!(
                                        "Could not connect to {}: {error}",
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

    pub(crate) fn open_token_creation_url(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let url = match self.setup_provider {
            ProviderChoice::Forgejo | ProviderChoice::Custom | ProviderChoice::AzureDevOps => {
                let raw = self.custom_host_input.read(cx).value().trim().to_string();
                let Ok(parsed) = url::Url::parse(&raw) else {
                    self.auth_status_message = Some("Enter a valid provider address first.".into());
                    cx.notify();
                    return;
                };
                if parsed.scheme() != "https"
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.query().is_some()
                    || parsed.fragment().is_some()
                {
                    self.auth_status_message =
                        Some("Enter a clean HTTPS provider address first.".into());
                    cx.notify();
                    return;
                }
                let suffix = if self.setup_provider == ProviderChoice::AzureDevOps {
                    "/_usersSettings/tokens"
                } else {
                    "/user/settings/applications"
                };
                format!("{}{suffix}", raw.trim_end_matches('/'))
            }
            _ => self.setup_provider.token_creation_url().to_string(),
        };
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).spawn();
        #[cfg(not(target_os = "macos"))]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        self.auth_status_message = Some("Opened provider token creation page in browser.".into());
        cx.notify();
    }
}
