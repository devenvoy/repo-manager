//! Project setup wizard, project management, and directory coordination controller.

use crate::app::ReleaseDesk;
use crate::settings;
use crate::workspace::{
    AuthModeChoice, ProjectSection, ProviderChoice, WorkspaceProject, expand_directory,
};
use crate::workspace_sources::RepositoryListMode;
use gpui_kit::{Context, PathPromptOptions, Window};
use release_desk_core::project::DiskLayout;

impl ReleaseDesk {
    pub(crate) fn begin_setup(
        &mut self,
        editing: Option<u128>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let original = editing
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
            .cloned();
        self.editing_project_id = editing;
        self.setup_step = 0;
        self.setup_active = true;
        self.setup_provider = original
            .as_ref()
            .map_or(ProviderChoice::GitHub, |project| project.provider);
        self.setup_layout = original
            .as_ref()
            .map_or(DiskLayout::Grouped, |project| project.disk_layout);
        let name = original
            .as_ref()
            .map_or("", |project| project.name.as_str())
            .to_string();
        let directory = original.as_ref().map_or(String::new(), |project| {
            project.directory.display().to_string()
        });
        let account = original
            .as_ref()
            .map_or("", |project| project.account.as_str())
            .to_string();
        let host = original
            .as_ref()
            .and_then(|project| project.custom_host.as_deref())
            .unwrap_or_default()
            .to_string();

        self.detected_system_credential = None;
        self.credential_discovery_revision = self.credential_discovery_revision.wrapping_add(1);
        if original
            .as_ref()
            .is_none_or(|project| project.credential_ref.is_none())
        {
            self.discover_setup_credential(false, cx);
        }

        if let Some(ref orig) = original {
            self.saved_credential_ref = orig.credential_ref.clone();
            self.selected_saved_account =
                orig.credential_ref.as_ref().map(|_| orig.account.clone());
            self.auth_mode_choice = if self.saved_credential_ref.is_some() {
                AuthModeChoice::SavedAccount
            } else if self.setup_provider == ProviderChoice::GitHub {
                AuthModeChoice::OAuthBrowser
            } else {
                AuthModeChoice::Token
            };
            self.auth_status_message = None;
        } else {
            self.selected_saved_account = None;
            self.saved_credential_ref = None;
            self.auth_mode_choice = if self.setup_provider == ProviderChoice::GitHub {
                AuthModeChoice::OAuthBrowser
            } else {
                AuthModeChoice::Token
            };
            self.auth_status_message = None;
        }

        let initial_account = if !account.is_empty() {
            account
        } else if let Some(ref sel) = self.selected_saved_account {
            sel.clone()
        } else {
            String::new()
        };

        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.directory_input
            .update(cx, |input, cx| input.set_value(directory, window, cx));
        self.account_input
            .update(cx, |input, cx| input.set_value(initial_account, window, cx));
        self.custom_host_input
            .update(cx, |input, cx| input.set_value(host, window, cx));
        self.oauth_client_id_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.token_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        cx.notify();
    }

    pub(crate) fn change_setup_provider(
        &mut self,
        provider: ProviderChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.setup_provider = provider;
        self.detected_system_credential = None;
        self.discover_setup_credential(false, cx);

        self.selected_saved_account = None;
        self.saved_credential_ref = None;
        self.auth_mode_choice = if provider == ProviderChoice::GitHub {
            AuthModeChoice::OAuthBrowser
        } else {
            AuthModeChoice::Token
        };
        self.auth_status_message = None;
        self.account_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.token_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.custom_host_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        cx.notify();
    }

    pub(crate) fn find_setup_credential(&mut self, cx: &mut Context<Self>) {
        self.discover_setup_credential(true, cx);
    }

    fn discover_setup_credential(&mut self, manual: bool, cx: &mut Context<Self>) {
        self.credential_discovery_revision = self.credential_discovery_revision.wrapping_add(1);
        let revision = self.credential_discovery_revision;
        if !manual
            && matches!(
                self.setup_provider,
                ProviderChoice::Forgejo | ProviderChoice::Custom
            )
        {
            self.detected_system_credential = None;
            return;
        }
        let host = if matches!(
            self.setup_provider,
            ProviderChoice::Forgejo | ProviderChoice::Custom
        ) {
            let raw = self.custom_host_input.read(cx).value().trim().to_string();
            url::Url::parse(&raw)
                .ok()
                .filter(|url| {
                    url.scheme() == "https" && url.username().is_empty() && url.password().is_none()
                })
                .and_then(|url| url.host_str().map(str::to_string))
                .unwrap_or_default()
        } else {
            self.setup_provider.default_host().to_string()
        };
        if host.is_empty() || self.setup_provider == ProviderChoice::AwsCodeCommit {
            self.detected_system_credential = None;
            if manual {
                self.auth_status_message = Some(
                    "Enter a valid HTTPS server address before checking Git credentials.".into(),
                );
                cx.notify();
            }
            return;
        }
        if manual {
            self.auth_status_message = Some("Looking for an existing Git credential…".into());
            cx.notify();
        }
        let secret_store = self.secret_store.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let credential = if manual {
                release_desk_store::credentials::OsSecretStore::discover_system_git_credentials(
                    &host,
                )
            } else {
                secret_store.discover_cached_system_git_credentials(&host)
            };
            let _ = tx.send(credential);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(credential) => {
                        if let Some(this) = this.upgrade() {
                            this.update(cx, |this, cx| {
                                if this.setup_active
                                    && this.credential_discovery_revision == revision
                                {
                                    this.detected_system_credential = credential;
                                    if manual {
                                        this.auth_status_message = Some(if this.detected_system_credential.is_some() {
                                            "Found an existing Git credential. Use the account shown below to connect.".into()
                                        } else {
                                            "No existing Git credential was found for this host.".into()
                                        });
                                    }
                                    cx.notify();
                                }
                            });
                        }
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }

    pub(crate) fn setup_back(&mut self, cx: &mut Context<Self>) {
        if self.setup_step == 0 {
            self.setup_active = false;
            self.editing_project_id = None;
            self.selected_saved_account = None;
            self.saved_credential_ref = None;
            self.auth_status_message = None;
        } else {
            self.setup_step -= 1;
        }
        cx.notify();
    }

    pub(crate) fn browse_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose project folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.directory_input.update(cx, |input, cx| {
                            input.set_value(path.display().to_string(), window, cx);
                        });
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn setup_next(&mut self, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).value().trim().to_string();
        let directory = expand_directory(self.directory_input.read(cx).value().as_ref());
        let host = self.custom_host_input.read(cx).value().trim().to_string();

        if self.setup_step == 0 && (name.is_empty() || !directory.is_dir()) {
            self.message = "Enter a name and choose an existing directory.".into();
            cx.notify();
            return;
        }
        if self.setup_step == 1 && self.setup_provider.requires_endpoint() && host.is_empty() {
            self.message = format!("Enter {}.", self.setup_provider.endpoint_label());
            cx.notify();
            return;
        }
        if self.setup_step < 1 {
            self.setup_step += 1;
            self.message.clear();
            cx.notify();
            return;
        }

        let id = self.editing_project_id.unwrap_or_else(|| {
            self.projects
                .iter()
                .map(|project| project.id)
                .max()
                .unwrap_or(0)
                + 1
        });
        let account = self.account_input.read(cx).value().trim().to_string();
        let token = self.token_input.read(cx).value().trim().to_string();
        if !token.is_empty() && self.saved_credential_ref.is_none() {
            self.message =
                "Click Save Token to validate this credential before creating the project.".into();
            cx.notify();
            return;
        }
        let previous = self
            .editing_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id));
        if let Some(previous) = previous {
            if previous.provider == self.setup_provider
                && previous.provider.requires_endpoint()
                && previous.custom_host.as_deref() != Some(host.as_str())
                && self.saved_credential_ref == previous.credential_ref
                && self.saved_credential_ref.is_some()
            {
                self.message =
                    "Provider address changed. Connect a token for the new address or disconnect the old credential."
                        .into();
                cx.notify();
                return;
            }
        }
        let credential_ref = self.saved_credential_ref.clone();
        if let Some(reference) = credential_ref.as_deref() {
            let expected = release_desk_store::provider_sources::connection::credential_key(
                self.setup_provider.as_str(),
                &account,
                self.setup_provider
                    .requires_endpoint()
                    .then_some(host.as_str()),
            );
            let unchanged_legacy = previous.is_some_and(|project| {
                project.provider == self.setup_provider
                    && project.account == account
                    && project.custom_host.as_deref()
                        == self
                            .setup_provider
                            .requires_endpoint()
                            .then_some(host.as_str())
                    && project.credential_ref.as_deref() == Some(reference)
            });
            let managed_browser = self
                .setup_provider
                .credential_manager_reference(
                    self.setup_provider
                        .requires_endpoint()
                        .then_some(host.as_str()),
                )
                .as_deref()
                == Some(reference);
            if reference != expected && !unchanged_legacy && !managed_browser {
                self.message =
                    "The saved credential belongs to another account or provider address. Connect again."
                        .into();
                cx.notify();
                return;
            }
        }
        let project = WorkspaceProject {
            id,
            name,
            directory,
            provider: self.setup_provider,
            account,
            custom_host: self.setup_provider.requires_endpoint().then_some(host),
            credential_ref,
            disk_layout: self.setup_layout,
            sources: self
                .editing_project_id
                .and_then(|id| self.projects.iter().find(|project| project.id == id))
                .filter(|project| project.provider == self.setup_provider)
                .map_or_else(Vec::new, |project| project.sources.clone()),
            repository_list_mode: self
                .editing_project_id
                .and_then(|id| self.projects.iter().find(|project| project.id == id))
                .map_or(RepositoryListMode::Grouped, |project| {
                    project.repository_list_mode
                }),
        };
        if let Err(error) = project.validate() {
            self.message = error;
            cx.notify();
            return;
        }
        if self
            .projects
            .iter()
            .any(|other| other.id != id && other.directory == project.directory)
        {
            self.message = "That directory already belongs to another project.".into();
            cx.notify();
            return;
        }
        let mut updated = self.settings.clone();
        if let Some(existing) = updated
            .projects
            .iter_mut()
            .find(|existing| existing.id == id)
        {
            *existing = project;
        } else {
            updated.projects.push(project);
        }
        if let Err(error) = settings::save(&updated) {
            self.message = error;
            cx.notify();
            return;
        }
        self.projects = updated.projects.clone();
        self.settings = updated;
        self.selected_project_id = Some(id);
        self.project_section = ProjectSection::Overview;
        self.setup_active = false;
        self.editing_project_id = None;
        self.message = "Project saved on this computer.".into();
        cx.notify();
    }

    pub(crate) fn delete_selected_project(&mut self, cx: &mut Context<Self>) {
        if self.project_section != ProjectSection::Settings {
            return;
        }
        if !self.delete_confirm {
            self.delete_confirm = true;
            cx.notify();
            return;
        }
        let Some(id) = self.selected_project_id else {
            return;
        };
        let mut updated = self.settings.clone();
        updated.projects.retain(|project| project.id != id);
        if let Err(error) = settings::save(&updated) {
            self.message = error;
            cx.notify();
            return;
        }
        self.projects = updated.projects.clone();
        self.settings = updated;
        self.selected_project_id = None;
        self.delete_confirm = false;
        self.message = "Project removed. Files on disk were left in place.".into();
        cx.notify();
    }

    pub(crate) fn change_selected_project(
        &mut self,
        change: impl FnOnce(&mut WorkspaceProject),
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_project_id else {
            return;
        };
        let mut updated = self.settings.clone();
        let Some(project) = updated.projects.iter_mut().find(|project| project.id == id) else {
            return;
        };
        change(project);
        self.workspace_revision = self.workspace_revision.wrapping_add(1);
        self.git_statuses.clear();
        self.selected_repository = None;
        self.languages_expanded.clear();
        self.language_breakdowns.clear();
        self.release_state = Default::default();
        if let Err(error) = settings::save(&updated) {
            self.message = error;
        } else {
            self.projects = updated.projects.clone();
            self.settings = updated;
            self.message = "Project choices saved on this computer.".into();
        }
        cx.notify();
    }
}
