//! Project setup wizard, project management, and directory coordination controller.

use crate::app::ReleaseDesk;
use crate::settings;
use crate::workspace::{
    AuthModeChoice, ProjectSection, ProviderChoice, WorkspaceProject, expand_directory,
};
use crate::workspace_sources::RepositoryListMode;
use gpui_kit::{Context, PathPromptOptions, Window};
use release_desk_core::port::SecretStorePort as _;
use release_desk_core::project::DiskLayout;
use release_desk_store::credentials::OsSecretStore;

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
        if let Some(ref mut server) = self.local_oauth_server {
            server.stop();
        }
        self.local_oauth_server = None;
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

        self.detected_system_credential =
            OsSecretStore::discover_system_git_credentials(self.setup_provider.default_host());

        if let Some(ref orig) = original {
            self.saved_credential_ref = orig.credential_ref.clone();
            self.selected_saved_account = Some(orig.account.clone());
            self.auth_mode_choice = AuthModeChoice::SavedAccount;
            self.auth_status_message = None;
        } else {
            self.selected_saved_account = None;
            self.saved_credential_ref = None;
            self.auth_mode_choice = AuthModeChoice::OAuthBrowser;
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
        if let Some(ref mut server) = self.local_oauth_server {
            server.stop();
        }
        self.local_oauth_server = None;
        self.setup_provider = provider;
        self.detected_system_credential =
            OsSecretStore::discover_system_git_credentials(provider.default_host());

        self.selected_saved_account = None;
        self.saved_credential_ref = None;
        self.auth_mode_choice = AuthModeChoice::OAuthBrowser;
        self.auth_status_message = None;
        self.account_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.token_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.custom_host_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        cx.notify();
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
        if self.setup_step == 1 && self.setup_provider == ProviderChoice::Custom && host.is_empty()
        {
            self.message = "Enter a server URL for Custom Git.".into();
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
        let credential_ref = if let Some(ref saved_ref) = self.saved_credential_ref {
            Some(saved_ref.clone())
        } else if !token.is_empty() {
            let key_suffix = if !account.is_empty() {
                account.as_str()
            } else {
                "default"
            };
            let cred_key = format!(
                "release_desk_{}_{}",
                self.setup_provider.as_str(),
                key_suffix
            );
            if let Err(error) = self.secret_store.set_secret(&cred_key, &token) {
                self.message = format!("Could not save token: {error}");
                cx.notify();
                return;
            }
            Some(cred_key)
        } else if let Some(existing) = self
            .editing_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
        {
            existing.credential_ref.clone()
        } else {
            None
        };
        let project = WorkspaceProject {
            id,
            name,
            directory,
            provider: self.setup_provider,
            account,
            custom_host: (self.setup_provider == ProviderChoice::Custom).then_some(host),
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
