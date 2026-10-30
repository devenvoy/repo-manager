//! Project authentication credentials coordination.

use super::*;

impl ReleaseDesk {
    pub(super) fn connect_credential_manager(
        &mut self,
        host: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let provider = self.setup_provider;
        let endpoint = self.custom_host_input.read(cx).value().trim().to_string();
        let Some(reference) = provider
            .credential_manager_reference((!endpoint.is_empty()).then_some(endpoint.as_str()))
        else {
            self.auth_status_message =
                Some("Enter a valid provider address before browser sign-in.".into());
            cx.notify();
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_status_message = Some(format!(
            "Waiting for {} browser sign-in through Git Credential Manager…",
            provider.label()
        ));
        std::thread::spawn(move || {
            let result = (|| {
                let path = (provider == ProviderChoice::AzureDevOps).then(|| {
                    endpoint
                        .trim_start_matches("https://dev.azure.com/")
                        .trim_matches('/')
                });
                let (_, token) = OsSecretStore::credential_manager_account(host, path)
                    .map_err(|error| error.to_string())?;
                let login = release_desk_store::provider_sources::connection::validate_token(
                    provider.as_str(),
                    &token,
                    (!endpoint.is_empty()).then_some(endpoint.as_str()),
                )
                .map_err(|error| error.to_string())?;
                Ok::<_, String>(login)
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
                                Ok(login) => {
                                    this.saved_credential_ref = Some(reference);
                                    this.selected_saved_account = Some(login.clone());
                                    this.auth_mode_choice = AuthModeChoice::SavedAccount;
                                    this.account_input.update(cx, |input, cx| {
                                        input.set_value(login.clone(), window, cx)
                                    });
                                    this.auth_status_message = Some(format!(
                                        "Connected to {} as @{login} through Git Credential Manager.",
                                        provider.label()
                                    ));
                                }
                                Err(error) => {
                                    this.auth_status_message = Some(format!(
                                        "Browser sign-in could not complete: {error}"
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

    pub(crate) fn connect_system_credential(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, token)) = self.detected_system_credential.clone() else {
            return;
        };
        let provider = self.setup_provider;
        let endpoint = self.custom_host_input.read(cx).value().trim().to_string();
        let secret_store = self.secret_store.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.auth_status_message = Some("Checking system credential with provider…".into());
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
                                }
                                Err(error) => {
                                    this.auth_status_message = Some(format!(
                                        "System credential is not valid for {}: {error}",
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

    pub(crate) fn select_saved_account(
        &mut self,
        handle: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let endpoint = self.custom_host_input.read(cx).value().trim().to_string();
        let Some(cred_key) = self
            .projects
            .iter()
            .find(|project| {
                project.provider == self.setup_provider
                    && project.account == handle
                    && (!project.provider.requires_endpoint()
                        || project.custom_host.as_deref() == Some(endpoint.as_str()))
            })
            .and_then(|project| project.credential_ref.clone())
        else {
            self.message = "No saved credential belongs to this provider address.".into();
            cx.notify();
            return;
        };
        let secret_store = self.secret_store.clone();
        let provider = self.setup_provider;
        let (tx, rx) = std::sync::mpsc::channel();
        self.message = "Checking saved account…".into();
        cx.notify();
        let key_for_lookup = cred_key.clone();
        std::thread::spawn(move || {
            let secret = if key_for_lookup.starts_with("gcm:") {
                if provider
                    .credential_manager_reference(
                        (!endpoint.is_empty()).then_some(endpoint.as_str()),
                    )
                    .as_deref()
                    != Some(key_for_lookup.as_str())
                {
                    Err("Saved browser account belongs to another provider".to_string())
                } else {
                    let host = provider
                        .credential_manager_host()
                        .expect("matching managed reference has a host");
                    let path = (provider == ProviderChoice::AzureDevOps).then(|| {
                        endpoint
                            .trim_start_matches("https://dev.azure.com/")
                            .trim_matches('/')
                    });
                    OsSecretStore::credential_manager_account(host, path)
                        .map(|(_, token)| Some(token))
                        .map_err(|error| error.to_string())
                }
            } else {
                secret_store
                    .get_secret(&key_for_lookup)
                    .map_err(|error| error.to_string())
            };
            let result = secret.and_then(|secret| {
                if let Some(ref token) = secret {
                    release_desk_store::provider_sources::connection::validate_token(
                        provider.as_str(),
                        token,
                        (!endpoint.is_empty()).then_some(endpoint.as_str()),
                    )
                    .map_err(|error| error.to_string())?;
                }
                Ok(secret)
            });
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
                                Ok(Some(_)) => {
                                    this.selected_saved_account = Some(handle.clone());
                                    this.saved_credential_ref = Some(cred_key);
                                    this.auth_mode_choice = AuthModeChoice::SavedAccount;
                                    this.auth_status_message = Some(format!(
                                        "Using validated Keychain credentials for @{handle}."
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
}
