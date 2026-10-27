//! Release Desk native desktop shell.

use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, PathPromptOptions, Render,
    Styled as _, Window, WindowOptions, div,
};
use release_desk_application::event::EventBroadcaster;
use release_desk_application::use_case::auth::AuthenticateProviderUseCase;
use release_desk_core::app_catalog::{AppActionState, AppStoreItem};
use release_desk_core::port::SecretStorePort as _;
use release_desk_core::project::DiskLayout;
use release_desk_store::auth::{
    LocalOAuthServer, ModalWebviewAuthPort, OAuthRedirectOutcome, SystemBrowserAuthPort,
};
use release_desk_store::credentials::OsSecretStore;
use repo_manager::components::{render_header, render_sidebar};
use repo_manager::settings::{self, Appearance, Settings, StartView};
use repo_manager::state::WorkspaceTab;
use repo_manager::views;
use repo_manager::workspace::{
    AuthModeChoice, ProjectSection, ProviderChoice, WorkspaceProject, expand_directory,
    find_saved_accounts_for_provider,
};
use repo_manager::workspace_sources::{RepositoryListMode, WorkspaceSource};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
enum NavLocation {
    ProjectHub,
    Project(u128, ProjectSection),
    AppStore,
    AppDetail(String),
    Settings,
}

struct ReleaseDesk {
    settings: Settings,
    projects: Vec<WorkspaceProject>,
    tab: WorkspaceTab,
    selected_project_id: Option<u128>,
    project_section: ProjectSection,
    delete_confirm: bool,
    setup_active: bool,
    setup_step: usize,
    editing_project_id: Option<u128>,
    setup_provider: ProviderChoice,
    setup_layout: DiskLayout,
    name_input: Entity<InputState>,
    directory_input: Entity<InputState>,
    account_input: Entity<InputState>,
    custom_host_input: Entity<InputState>,
    oauth_client_id_input: Entity<InputState>,
    token_input: Entity<InputState>,
    source_input: Entity<InputState>,
    store_items: Vec<AppStoreItem>,
    selected_app_id: Option<String>,
    reviewing_clones: bool,
    selected_platform: &'static str,
    message: String,
    nav_history: Vec<NavLocation>,
    nav_index: usize,
    secret_store: Arc<OsSecretStore>,
    #[allow(dead_code)]
    auth_uc: AuthenticateProviderUseCase,
    selected_saved_account: Option<String>,
    saved_credential_ref: Option<String>,
    auth_mode_choice: AuthModeChoice,
    auth_status_message: Option<String>,
    detected_system_credential: Option<(String, String)>,
    local_oauth_server: Option<LocalOAuthServer>,
}

impl ReleaseDesk {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (settings, message) = match settings::load() {
            Ok(settings) => (settings, String::new()),
            Err(error) => (Settings::default(), error),
        };
        let projects = settings.projects.clone();
        let (tab, initial_loc) = match settings.start_view {
            StartView::ProjectHub => (WorkspaceTab::ProjectHub, NavLocation::ProjectHub),
            StartView::AppStore => (WorkspaceTab::AppStore, NavLocation::AppStore),
        };
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. Product Suite"));
        let directory_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/project/directory"));
        let account_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Account or organization"));
        let custom_host_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://git.example.com"));
        let oauth_client_id_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("e.g. 178c6fc7... (optional)"));
        let token_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Personal access token"));
        let source_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Handle or profile URL"));
        let secret_store = Arc::new(OsSecretStore::new());
        let browser_auth = Arc::new(SystemBrowserAuthPort::new());
        let webview_auth = Arc::new(ModalWebviewAuthPort::new());
        let auth_uc = AuthenticateProviderUseCase::new(
            secret_store.clone(),
            browser_auth.clone(),
            webview_auth.clone(),
            EventBroadcaster::default(),
        );
        let detected_system_credential =
            OsSecretStore::discover_system_git_credentials("github.com");
        let app = Self {
            settings,
            projects,
            tab,
            selected_project_id: None,
            project_section: ProjectSection::Overview,
            delete_confirm: false,
            setup_active: false,
            setup_step: 0,
            editing_project_id: None,
            setup_provider: ProviderChoice::GitHub,
            setup_layout: DiskLayout::Grouped,
            name_input,
            directory_input,
            account_input,
            custom_host_input,
            oauth_client_id_input,
            token_input,
            source_input,
            store_items: Vec::new(),
            selected_app_id: None,
            reviewing_clones: false,
            selected_platform: "macOS",
            message,
            nav_history: vec![initial_loc],
            nav_index: 0,
            secret_store,
            auth_uc,
            selected_saved_account: None,
            saved_credential_ref: None,
            auth_mode_choice: AuthModeChoice::OAuthBrowser,
            auth_status_message: None,
            detected_system_credential,
            local_oauth_server: None,
        };
        app.apply_appearance(window, cx);
        app
    }

    fn navigate_to(&mut self, loc: NavLocation, cx: &mut Context<Self>) {
        if self.nav_index < self.nav_history.len() && self.nav_history[self.nav_index] == loc {
            return;
        }
        if self.nav_index + 1 < self.nav_history.len() {
            self.nav_history.truncate(self.nav_index + 1);
        }
        self.nav_history.push(loc.clone());
        self.nav_index = self.nav_history.len() - 1;
        self.apply_nav_location(loc);
        cx.notify();
    }

    fn apply_nav_location(&mut self, loc: NavLocation) {
        match loc {
            NavLocation::ProjectHub => {
                self.tab = WorkspaceTab::ProjectHub;
                self.selected_project_id = None;
                self.setup_active = false;
            }
            NavLocation::Project(id, section) => {
                self.tab = WorkspaceTab::ProjectHub;
                self.selected_project_id = Some(id);
                self.project_section = section;
                self.setup_active = false;
            }
            NavLocation::AppStore => {
                self.tab = WorkspaceTab::AppStore;
                self.selected_app_id = None;
            }
            NavLocation::AppDetail(id) => {
                self.tab = WorkspaceTab::AppStore;
                self.selected_app_id = Some(id);
            }
            NavLocation::Settings => {
                self.tab = WorkspaceTab::Settings;
                self.setup_active = false;
            }
        }
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.nav_index > 0 {
            self.nav_index -= 1;
            let loc = self.nav_history[self.nav_index].clone();
            self.apply_nav_location(loc);
            cx.notify();
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.nav_index + 1 < self.nav_history.len() {
            self.nav_index += 1;
            let loc = self.nav_history[self.nav_index].clone();
            self.apply_nav_location(loc);
            cx.notify();
        }
    }

    fn apply_appearance(&self, window: &Window, cx: &mut Context<Self>) {
        let mode = match self.settings.appearance {
            Appearance::System => ThemeMode::from(window.appearance()),
            Appearance::Light => ThemeMode::Light,
            Appearance::Dark => ThemeMode::Dark,
        };
        Theme::change(mode, None, cx);
        let is_dark = mode.is_dark();
        Theme::update(cx, |theme| {
            if is_dark {
                theme.background = gpui_kit::hsla(222.0 / 360.0, 0.40, 0.08, 1.0);
                theme.secondary = gpui_kit::hsla(222.0 / 360.0, 0.40, 0.05, 1.0);
                theme.border = gpui_kit::hsla(217.0 / 360.0, 0.25, 0.20, 1.0);
                theme.primary = gpui_kit::hsla(232.0 / 360.0, 0.85, 0.65, 1.0);
                theme.foreground = gpui_kit::hsla(210.0 / 360.0, 0.40, 0.98, 1.0);
                theme.muted_foreground = gpui_kit::hsla(215.0 / 360.0, 0.20, 0.65, 1.0);
                theme.muted = gpui_kit::hsla(222.0 / 360.0, 0.35, 0.14, 1.0);
            } else {
                theme.background = gpui_kit::hsla(220.0 / 360.0, 0.14, 0.97, 1.0);
                theme.secondary = gpui_kit::hsla(220.0 / 360.0, 0.18, 0.95, 1.0);
                theme.border = gpui_kit::hsla(216.0 / 360.0, 0.23, 0.91, 1.0);
                theme.primary = gpui_kit::hsla(232.0 / 360.0, 0.80, 0.56, 1.0);
                theme.foreground = gpui_kit::hsla(218.0 / 360.0, 0.37, 0.15, 1.0);
                theme.muted_foreground = gpui_kit::hsla(217.0 / 360.0, 0.14, 0.48, 1.0);
                theme.muted = gpui_kit::hsla(220.0 / 360.0, 0.25, 0.93, 1.0);
            }
        });
    }

    fn begin_setup(&mut self, editing: Option<u128>, window: &mut Window, cx: &mut Context<Self>) {
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

    fn change_setup_provider(
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
        cx.notify();
    }

    fn connect_system_credential(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((ref username, ref token)) = self.detected_system_credential {
            let cred_key = format!("release_desk_{}_{}", self.setup_provider.as_str(), username);
            let _ = self.secret_store.set_secret(&cred_key, token);
            self.saved_credential_ref = Some(cred_key);
            self.selected_saved_account = Some(username.clone());
            self.auth_mode_choice = AuthModeChoice::SavedAccount;
            self.auth_status_message = Some(format!(
                "✓ Connected with {} API as @{} (5,000 req/hr). Token stored in Keychain.",
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

    fn select_saved_account(
        &mut self,
        handle: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cred_key = format!("release_desk_{}_{}", self.setup_provider.as_str(), handle);
        self.selected_saved_account = Some(handle.clone());
        self.saved_credential_ref = Some(cred_key);
        self.auth_mode_choice = AuthModeChoice::SavedAccount;
        self.auth_status_message = Some(format!(
            "✓ Using Keychain credentials for @{}. No token needed!",
            handle
        ));
        self.account_input
            .update(cx, |input, cx| input.set_value(handle, window, cx));
        cx.notify();
    }

    fn disconnect_credential(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref mut server) = self.local_oauth_server {
            server.stop();
        }
        self.local_oauth_server = None;
        self.saved_credential_ref = None;
        self.selected_saved_account = None;
        self.auth_status_message = None;
        self.account_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        self.token_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        cx.notify();
    }

    fn select_auth_tab(&mut self, mode: AuthModeChoice, cx: &mut Context<Self>) {
        self.auth_mode_choice = mode;
        cx.notify();
    }

    fn trigger_browser_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref mut server) = self.local_oauth_server {
            server.stop();
        }
        self.local_oauth_server = None;

        match LocalOAuthServer::start(4567) {
            Ok(server) => {
                let redirect_uri = format!("http://127.0.0.1:{}/callback", server.port());
                let custom_client_id = self
                    .oauth_client_id_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let url = self
                    .setup_provider
                    .oauth_authorize_url(&custom_client_id, &redirect_uri);

                self.local_oauth_server = Some(server);
                self.auth_mode_choice = AuthModeChoice::OAuthBrowser;
                self.auth_status_message = Some(
                    "⏳ Listening on http://127.0.0.1:4567/callback... Waiting for browser authorization.".into(),
                );
                self.message = "Launched browser for OAuth. Listening on local port 4567.".into();

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
                            this.local_oauth_server.as_ref().and_then(|s| s.poll_outcome())
                        });

                        if let Ok(Some(outcome)) = outcome {
                            let _ = this.update_in(cx, |this, window, cx| {
                                match outcome {
                                    OAuthRedirectOutcome::Success { code } => {
                                        let cred_key = format!(
                                            "release_desk_{}_browser_oauth",
                                            this.setup_provider.as_str()
                                        );
                                        let _ = this.secret_store.set_secret(&cred_key, &code);
                                        this.saved_credential_ref = Some(cred_key);
                                        this.selected_saved_account =
                                            Some("browser-authorized".into());
                                        this.auth_status_message = Some(
                                            "✓ Successfully authenticated via browser (5,000 req/hr)."
                                                .into(),
                                        );
                                        this.message =
                                            "Browser authentication received successfully.".into();
                                        this.account_input.update(cx, |input, cx| {
                                            input.set_value(
                                                "browser-authorized",
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                    OAuthRedirectOutcome::Error { message } => {
                                        this.auth_status_message = Some(format!(
                                            "⚠️ Browser authentication failed: {message}"
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
                    "⚠️ Could not start local redirect server on port 4567 ({e}). Please use Token or In-App Webview."
                ));
                cx.notify();
            }
        }
    }

    fn cancel_browser_auth(&mut self, cx: &mut Context<Self>) {
        if let Some(ref mut server) = self.local_oauth_server {
            server.stop();
        }
        self.local_oauth_server = None;
        self.auth_status_message = None;
        cx.notify();
    }

    fn trigger_webview_auth(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let url = self
            .setup_provider
            .oauth_authorize_url("", "http://127.0.0.1:4567/callback");
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(url).spawn();
        #[cfg(not(target_os = "macos"))]
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();

        self.auth_mode_choice = AuthModeChoice::OAuthWebview;
        self.auth_status_message =
            Some("Launched in-app modal webview window. Complete sign in to authorize.".into());
        self.message = "Launched in-app webview window.".into();
        cx.notify();
    }

    fn save_token_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.token_input.read(cx).value().trim().to_string();
        if token.is_empty() {
            self.message = "Please enter a valid personal access token.".into();
            cx.notify();
            return;
        }
        let cred_key = format!("release_desk_{}_pat", self.setup_provider.as_str());
        let _ = self.secret_store.set_secret(&cred_key, &token);
        self.saved_credential_ref = Some(cred_key);
        self.selected_saved_account = Some("token-user".into());
        self.auth_status_message =
            Some("✓ Personal Access Token saved securely in macOS Keychain (5,000 req/hr).".into());
        self.message = "Personal Access Token stored in Keychain.".into();
        self.account_input.update(cx, |input, cx| {
            input.set_value("token-user", window, cx);
        });
        cx.notify();
    }

    fn open_token_creation_url(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let url = self.setup_provider.token_creation_url();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(url).spawn();
        #[cfg(not(target_os = "macos"))]
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        self.auth_status_message = Some("Opened provider token creation page in browser.".into());
        cx.notify();
    }

    fn setup_back(&mut self, cx: &mut Context<Self>) {
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

    fn browse_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn setup_next(&mut self, cx: &mut Context<Self>) {
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
        if self.setup_step < 2 {
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
            let _ = self.secret_store.set_secret(&cred_key, &token);
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

    fn delete_selected_project(&mut self, cx: &mut Context<Self>) {
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

    fn change_selected_project(
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
        if let Err(error) = settings::save(&updated) {
            self.message = error;
        } else {
            self.projects = updated.projects.clone();
            self.settings = updated;
            self.message = "Project choices saved on this computer.".into();
        }
        cx.notify();
    }

    fn add_source(&mut self, cx: &mut Context<Self>) {
        let raw = self.source_input.read(cx).value().to_string();
        match WorkspaceSource::sample(&raw) {
            Ok(source) => {
                if self
                    .selected_project_id
                    .and_then(|id| self.projects.iter().find(|project| project.id == id))
                    .is_some_and(|project| {
                        project
                            .sources
                            .iter()
                            .any(|existing| existing.handle == source.handle)
                    })
                {
                    self.message = "That source is already in this project.".into();
                    cx.notify();
                    return;
                }
                self.change_selected_project(|project| project.sources.push(source), cx);
            }
            Err(error) => {
                self.message = error;
                cx.notify();
            }
        }
    }

    fn remove_source(&mut self, source_index: usize, cx: &mut Context<Self>) {
        self.change_selected_project(
            |project| {
                if source_index < project.sources.len() {
                    project.sources.remove(source_index);
                }
            },
            cx,
        );
    }

    fn toggle_source_repo(
        &mut self,
        source_index: usize,
        repo_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.change_selected_project(
            |project| {
                if let Some(repo) = project
                    .sources
                    .get_mut(source_index)
                    .and_then(|source| source.repositories.get_mut(repo_index))
                {
                    repo.selected = !repo.selected;
                }
            },
            cx,
        );
    }

    fn set_repository_list_mode(&mut self, mode: RepositoryListMode, cx: &mut Context<Self>) {
        self.change_selected_project(|project| project.repository_list_mode = mode, cx);
    }

    fn preview_app_action(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(item) = self
            .store_items
            .iter_mut()
            .find(|item| item.metadata.id == id)
        else {
            return;
        };
        let action = match item.state {
            AppActionState::Installable => "installed",
            AppActionState::UpdateAvailable { .. } => "updated",
            AppActionState::UpToDate { .. } => "opened",
        };
        if action != "opened" {
            item.installed_version = item.latest_release_tag.clone();
            item.state = AppActionState::UpToDate {
                version: item.latest_release_tag.clone().unwrap_or_default(),
            };
        }
        self.message = format!("{} {action} in preview only.", item.metadata.name);
        cx.notify();
    }

    fn set_appearance(&mut self, appearance: Appearance, window: &Window, cx: &mut Context<Self>) {
        self.settings.appearance = appearance;
        let _ = settings::save(&self.settings);
        self.apply_appearance(window, cx);
        self.message = format!("Theme changed to {:?}", appearance);
        cx.notify();
    }

    fn select_all_source_repos(
        &mut self,
        source_index: usize,
        select: bool,
        cx: &mut Context<Self>,
    ) {
        self.change_selected_project(
            |project| {
                if let Some(source) = project.sources.get_mut(source_index) {
                    for repo in &mut source.repositories {
                        repo.selected = select;
                    }
                }
            },
            cx,
        );
    }

    fn execute_clone_plan(&mut self, count: usize, cx: &mut Context<Self>) {
        self.reviewing_clones = false;
        self.message = format!("Cloning planned and staged for {count} repositories.");
        cx.notify();
    }
}

impl Render for ReleaseDesk {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (location_title, project_name) = match self.tab {
            WorkspaceTab::AppStore => {
                if let Some(id) = &self.selected_app_id {
                    (id.as_str(), None)
                } else {
                    ("App Store", None)
                }
            }
            WorkspaceTab::Settings => ("Settings", None),
            WorkspaceTab::ProjectHub => {
                if self.setup_active {
                    if self.editing_project_id.is_some() {
                        ("Edit Project", None)
                    } else {
                        ("New Project", None)
                    }
                } else if let Some(project) = self
                    .selected_project_id
                    .and_then(|id| self.projects.iter().find(|p| p.id == id))
                {
                    (self.project_section.label(), Some(project.name.as_str()))
                } else {
                    ("Projects", None)
                }
            }
            _ => ("Workspace", None),
        };

        let content = match self.tab {
            WorkspaceTab::AppStore => views::render_store(
                &self.store_items,
                self.selected_app_id.as_deref(),
                cx,
                |id, this: &mut Self, cx| {
                    this.navigate_to(NavLocation::AppDetail(id), cx);
                },
                |id, this: &mut Self, cx| this.preview_app_action(id, cx),
                |this: &mut Self, cx| {
                    this.navigate_to(NavLocation::ProjectHub, cx);
                },
            )
            .into_any_element(),
            WorkspaceTab::Settings => views::render_settings(
                &self.settings,
                cx,
                |this: &mut Self, appearance, window, cx| {
                    this.set_appearance(appearance, window, cx);
                },
                |this: &mut Self, start_view, cx| {
                    this.settings.start_view = start_view;
                    let _ = settings::save(&this.settings);
                    this.message = format!("Startup view changed to {start_view:?}");
                    cx.notify();
                },
                |this: &mut Self, layout, cx| {
                    this.settings.hub_view_mode = layout;
                    let _ = settings::save(&this.settings);
                    this.message = format!("Default layout changed to {layout:?}");
                    cx.notify();
                },
                |this: &mut Self, cx| {
                    let _ = settings::save(&this.settings);
                    this.message = "Settings saved successfully.".into();
                    cx.notify();
                },
            )
            .into_any_element(),
            _ if self.setup_active => {
                let saved_accounts =
                    find_saved_accounts_for_provider(&self.projects, self.setup_provider);
                let has_existing_credential = self
                    .editing_project_id
                    .and_then(|id| self.projects.iter().find(|p| p.id == id))
                    .is_some_and(|p| p.credential_ref.is_some());

                views::render_project_setup(
                    views::ProjectSetupProps {
                        editing: self.editing_project_id.is_some(),
                        step: self.setup_step,
                        provider: self.setup_provider,
                        disk_layout: self.setup_layout,
                        name: &self.name_input,
                        directory: &self.directory_input,
                        custom_host: &self.custom_host_input,
                        oauth_client_id: &self.oauth_client_id_input,
                        token: &self.token_input,
                        system_credential: self.detected_system_credential.clone(),
                        saved_accounts,
                        selected_saved_account: self.selected_saved_account.clone(),
                        auth_mode: self.auth_mode_choice,
                        auth_status_message: self.auth_status_message.clone(),
                        has_existing_credential,
                        is_oauth_listening: self
                            .local_oauth_server
                            .as_ref()
                            .is_some_and(|s| s.is_listening()),
                    },
                    cx,
                    views::ProjectSetupActions {
                        on_provider: |this: &mut Self, provider, window, cx| {
                            this.change_setup_provider(provider, window, cx);
                        },
                        on_layout: |this: &mut Self, layout, cx: &mut Context<Self>| {
                            this.setup_layout = layout;
                            cx.notify();
                        },
                        on_next: |this: &mut Self, cx| this.setup_next(cx),
                        on_back: |this: &mut Self, cx| this.setup_back(cx),
                        on_browse: |this: &mut Self, window, cx| {
                            this.browse_directory(window, cx);
                        },
                        on_connect_system_credential: |this: &mut Self, window, cx| {
                            this.connect_system_credential(window, cx);
                        },
                        on_disconnect_credential: |this: &mut Self, window, cx| {
                            this.disconnect_credential(window, cx);
                        },
                        on_select_account: |this: &mut Self, handle, window, cx| {
                            this.select_saved_account(handle, window, cx);
                        },
                        on_select_auth_tab: |this: &mut Self, mode, cx| {
                            this.select_auth_tab(mode, cx);
                        },
                        on_auth_browser: |this: &mut Self, window, cx| {
                            this.trigger_browser_auth(window, cx);
                        },
                        on_cancel_browser_auth: |this: &mut Self, cx| {
                            this.cancel_browser_auth(cx);
                        },
                        on_auth_webview: |this: &mut Self, window, cx| {
                            this.trigger_webview_auth(window, cx);
                        },
                        on_save_token: |this: &mut Self, window, cx| {
                            this.save_token_input(window, cx);
                        },
                        on_open_token_url: |this: &mut Self, window, cx| {
                            this.open_token_creation_url(window, cx);
                        },
                    },
                )
                .into_any_element()
            }
            _ => {
                if let Some(project) = self
                    .selected_project_id
                    .and_then(|id| self.projects.iter().find(|project| project.id == id))
                {
                    let section_content = match self.project_section {
                        ProjectSection::Sources => {
                            if self.reviewing_clones {
                                let mut plans = Vec::new();
                                for source in &project.sources {
                                    for repo in &source.repositories {
                                        if repo.selected {
                                            let target_path =
                                                project.disk_layout.compute_clone_path(
                                                    &project.directory,
                                                    &source.handle,
                                                    &repo.name,
                                                );
                                            plans.push(
                                                release_desk_application::use_case::profile_ingestion::RepoClonePlan {
                                                    repo_name: repo.name.clone(),
                                                    clone_url: format!(
                                                        "https://{}/{}/{}.git",
                                                        project.provider.label().to_lowercase(),
                                                        source.handle,
                                                        repo.name
                                                    ),
                                                    target_path,
                                                },
                                            );
                                        }
                                    }
                                }
                                let count = plans.len();
                                Some(
                                    views::render_clone_review_modal(
                                        &project.name,
                                        &project.directory,
                                        project.disk_layout,
                                        &plans,
                                        cx,
                                        move |this: &mut Self, cx| {
                                            this.execute_clone_plan(count, cx)
                                        },
                                        |this: &mut Self, cx| {
                                            this.reviewing_clones = false;
                                            cx.notify();
                                        },
                                    )
                                    .into_any_element(),
                                )
                            } else {
                                Some(
                                    views::render_project_sources(
                                        &project.sources,
                                        &self.source_input,
                                        cx,
                                        |this: &mut Self, cx| this.add_source(cx),
                                        |index, this: &mut Self, cx| this.remove_source(index, cx),
                                        |source, repo, this: &mut Self, cx| {
                                            this.toggle_source_repo(source, repo, cx)
                                        },
                                        |source_index, select, this: &mut Self, cx| {
                                            this.select_all_source_repos(source_index, select, cx);
                                        },
                                        |this: &mut Self, cx| {
                                            this.reviewing_clones = true;
                                            cx.notify();
                                        },
                                    )
                                    .into_any_element(),
                                )
                            }
                        }
                        ProjectSection::Repositories => Some(
                            views::render_project_repositories(
                                &project.sources,
                                project.repository_list_mode,
                                cx,
                                |mode, this: &mut Self, cx| this.set_repository_list_mode(mode, cx),
                            )
                            .into_any_element(),
                        ),
                        ProjectSection::Releases => Some(
                            views::render_project_releases(
                                project,
                                self.selected_platform,
                                cx,
                                |platform, this: &mut Self, cx| {
                                    this.selected_platform = platform;
                                    cx.notify();
                                },
                                |asset_name, this: &mut Self, cx| {
                                    this.message =
                                        format!("Staged {asset_name} in .release-desk/installers/");
                                    cx.notify();
                                },
                            )
                            .into_any_element(),
                        ),
                        _ => None,
                    };
                    views::render_project_workspace(
                        project,
                        self.project_section,
                        self.delete_confirm,
                        section_content,
                        cx,
                        |section, this: &mut Self, cx| {
                            this.project_section = section;
                            this.delete_confirm = false;
                            cx.notify();
                        },
                        |this: &mut Self, cx| {
                            this.navigate_to(NavLocation::ProjectHub, cx);
                        },
                        |this: &mut Self, window, cx| {
                            this.begin_setup(this.selected_project_id, window, cx)
                        },
                        |this: &mut Self, cx| this.delete_selected_project(cx),
                    )
                    .into_any_element()
                } else {
                    views::render_hub(
                        &self.projects,
                        cx,
                        |this: &mut Self, window, cx| this.begin_setup(None, window, cx),
                        |id, this: &mut Self, cx| {
                            this.navigate_to(
                                NavLocation::Project(id, ProjectSection::Overview),
                                cx,
                            );
                        },
                    )
                    .into_any_element()
                }
            }
        };

        div()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(render_sidebar(
                self.tab,
                self.projects.len(),
                cx,
                |this: &mut Self, tab, cx| {
                    let loc = match tab {
                        WorkspaceTab::ProjectHub => NavLocation::ProjectHub,
                        WorkspaceTab::AppStore => NavLocation::AppStore,
                        WorkspaceTab::Settings => NavLocation::Settings,
                        _ => NavLocation::ProjectHub,
                    };
                    this.navigate_to(loc, cx);
                },
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .child(render_header(
                        self.nav_index > 0,
                        self.nav_index + 1 < self.nav_history.len(),
                        location_title,
                        project_name,
                        &self.message,
                        cx,
                        |this: &mut Self, cx| this.go_back(cx),
                        |this: &mut Self, cx| this.go_forward(cx),
                        |this: &mut Self, cx| this.navigate_to(NavLocation::ProjectHub, cx),
                        |this: &mut Self, cx| {
                            if let Some(id) = this.selected_project_id {
                                this.navigate_to(
                                    NavLocation::Project(id, ProjectSection::Overview),
                                    cx,
                                );
                            }
                        },
                    ))
                    .child(content),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new(window, cx))
            })
            .expect("failed to open Release Desk window");
        });
}
