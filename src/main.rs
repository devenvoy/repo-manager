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
    DevicePollOutcome, GitHubDeviceFlow, LocalOAuthServer, ModalWebviewAuthPort,
    OAuthRedirectOutcome, SystemBrowserAuthPort, copy_to_macos_clipboard,
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
    source_search: Entity<InputState>,
    source_dialog: views::project_sources::SourceDialog,
    source_busy: bool,
    store_items: Vec<AppStoreItem>,
    selected_app_id: Option<String>,
    git_statuses: Vec<repo_manager::repository_git::RepositoryStatus>,
    selected_repository: Option<(usize, usize)>,
    git_busy: bool,
    workspace_revision: u64,
    release_state: repo_manager::workspace_releases::ReleaseViewState,
    release_busy: bool,
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
    device_flow_user_code: Option<String>,
    device_flow_verification_uri: Option<String>,
    device_flow_canceller: Option<Arc<std::sync::atomic::AtomicBool>>,
    active_webview_child: Option<std::process::Child>,
}

fn project_auth_token(
    project: &WorkspaceProject,
    secret_store: &OsSecretStore,
) -> Result<Option<String>, String> {
    let Some(reference) = &project.credential_ref else {
        return Ok(None);
    };
    match secret_store.get_secret(reference) {
        Ok(Some(token)) if !token.is_empty() => Ok(Some(token)),
        Ok(_) => Err("Selected project credential is missing. Connect the account again.".into()),
        Err(error) => Err(format!("Cannot read selected project credential: {error}")),
    }
}

fn find_webview_binary() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("release-desk-webview");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let local = std::path::PathBuf::from("bin/release-desk-webview");
    if local.is_file() {
        return Some(local);
    }
    let target = std::path::PathBuf::from("target/release/release-desk-webview");
    if target.is_file() {
        return Some(target);
    }
    None
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
        let source_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search repositories"));
        cx.observe(&source_search, |_, _, cx| cx.notify()).detach();
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
            source_search,
            source_dialog: Default::default(),
            source_busy: false,
            store_items: Vec::new(),
            selected_app_id: None,
            git_statuses: Vec::new(),
            selected_repository: None,
            git_busy: false,
            workspace_revision: 0,
            release_state: Default::default(),
            release_busy: false,
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
            device_flow_user_code: None,
            device_flow_verification_uri: None,
            device_flow_canceller: None,
            active_webview_child: None,
        };
        app.apply_appearance(window, cx);
        let avatar_requests: Vec<_> = app
            .projects
            .iter()
            .flat_map(|project| &project.sources)
            .filter_map(WorkspaceSource::avatar_cache_request)
            .filter(|(_, path)| !path.is_file())
            .collect();
        if !avatar_requests.is_empty() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for (url, path) in avatar_requests {
                    let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path);
                }
                let _ = tx.send(());
            });
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(100))
                        .await;
                    if rx.try_recv().is_ok() {
                        let _ = this.update_in(cx, |_, _, cx| cx.notify());
                        return;
                    }
                }
            })
            .detach();
        }
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
        let next_project = match &loc {
            NavLocation::Project(id, _) => Some(*id),
            _ => None,
        };
        if next_project != self.selected_project_id {
            self.workspace_revision = self.workspace_revision.wrapping_add(1);
            self.git_statuses.clear();
            self.selected_repository = None;
            self.release_state = Default::default();
            self.source_dialog = views::project_sources::SourceDialog::Closed;
        }
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
                theme.background = gpui_kit::hsla(216.0 / 360.0, 0.23, 0.086, 1.0);
                theme.secondary = gpui_kit::hsla(219.0 / 360.0, 0.21, 0.13, 1.0);
                theme.border = gpui_kit::hsla(218.0 / 360.0, 0.20, 0.22, 1.0);
                theme.primary = gpui_kit::hsla(232.0 / 360.0, 1.0, 0.78, 1.0);
                theme.primary_foreground = gpui_kit::hsla(225.0 / 360.0, 0.32, 0.09, 1.0);
                theme.foreground = gpui_kit::hsla(215.0 / 360.0, 0.22, 0.93, 1.0);
                theme.muted_foreground = gpui_kit::hsla(216.0 / 360.0, 0.17, 0.65, 1.0);
                theme.muted = gpui_kit::hsla(229.0 / 360.0, 0.35, 0.21, 1.0);
                theme.success = gpui_kit::hsla(150.0 / 360.0, 0.58, 0.60, 1.0);
                theme.warning = gpui_kit::hsla(38.0 / 360.0, 0.86, 0.62, 1.0);
                theme.danger = gpui_kit::hsla(5.0 / 360.0, 1.0, 0.75, 1.0);
            } else {
                theme.background = gpui_kit::hsla(216.0 / 360.0, 0.23, 0.96, 1.0);
                theme.secondary = gpui_kit::hsla(0.0, 0.0, 1.0, 1.0);
                theme.border = gpui_kit::hsla(218.0 / 360.0, 0.20, 0.87, 1.0);
                theme.primary = gpui_kit::hsla(232.0 / 360.0, 0.72, 0.52, 1.0);
                theme.primary_foreground = gpui_kit::hsla(0.0, 0.0, 1.0, 1.0);
                theme.foreground = gpui_kit::hsla(218.0 / 360.0, 0.28, 0.12, 1.0);
                theme.muted_foreground = gpui_kit::hsla(216.0 / 360.0, 0.14, 0.40, 1.0);
                theme.muted = gpui_kit::hsla(232.0 / 360.0, 0.73, 0.94, 1.0);
                theme.success = gpui_kit::hsla(151.0 / 360.0, 0.68, 0.28, 1.0);
                theme.warning = gpui_kit::hsla(35.0 / 360.0, 1.0, 0.30, 1.0);
                theme.danger = gpui_kit::hsla(5.0 / 360.0, 0.71, 0.40, 1.0);
            }
            theme.button = theme.secondary;
            theme.button_foreground = theme.foreground;
            theme.button_primary = theme.primary;
            theme.button_primary_foreground = theme.primary_foreground;
            theme.button_primary_hover =
                gpui_kit::hsla(232.0 / 360.0, 0.90, if is_dark { 0.83 } else { 0.47 }, 1.0);
            theme.button_primary_active =
                gpui_kit::hsla(232.0 / 360.0, 0.90, if is_dark { 0.70 } else { 0.40 }, 1.0);
            theme.tokens.button_primary = theme.button_primary.into();
            theme.tokens.button_primary_hover = theme.button_primary_hover.into();
            theme.tokens.button_primary_active = theme.button_primary_active.into();
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

    fn select_saved_account(
        &mut self,
        handle: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cred_key = format!("release_desk_{}_{}", self.setup_provider.as_str(), handle);
        match self.secret_store.get_secret(&cred_key) {
            Ok(Some(_)) => {}
            Ok(None) => {
                self.message = "Saved account credential is missing. Connect again.".into();
                cx.notify();
                return;
            }
            Err(error) => {
                self.message = format!("Cannot read saved account: {error}");
                cx.notify();
                return;
            }
        }
        self.selected_saved_account = Some(handle.clone());
        self.saved_credential_ref = Some(cred_key);
        self.auth_mode_choice = AuthModeChoice::SavedAccount;
        self.auth_status_message = Some(format!(
            "Using Keychain credentials for @{}. No token needed!",
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

    fn copy_user_code(&mut self, code: String, cx: &mut Context<Self>) {
        copy_to_macos_clipboard(&code);
        self.message = format!("Code {code} copied to clipboard.");
        cx.notify();
    }

    fn trigger_browser_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn cancel_browser_auth(&mut self, cx: &mut Context<Self>) {
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

    fn trigger_webview_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn save_token_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn add_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_busy {
            return;
        }
        let raw = self.source_input.read(cx).value().trim().to_string();
        if raw.is_empty() {
            self.message = "Enter a user handle, organization, or repository URL.".into();
            cx.notify();
            return;
        }

        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
        else {
            self.message = "No project selected.".into();
            cx.notify();
            return;
        };

        let default_provider = project.provider.as_str();
        let (provider, owner, _) =
            release_desk_store::source_fetcher::parse_input_target(&raw, default_provider);

        if owner.is_empty()
            || owner.contains(['?', '#', ' ', '\\'])
            || owner.chars().any(char::is_control)
        {
            self.message = "Enter a valid user or organization handle, or profile URL.".into();
            cx.notify();
            return;
        }

        if let Some(existing_index) = project.sources.iter().position(|existing| {
            existing.provider.eq_ignore_ascii_case(&provider)
                && existing.handle.eq_ignore_ascii_case(&owner)
        }) {
            self.source_dialog = views::project_sources::SourceDialog::Select(existing_index);
            self.message = format!(
                "@{owner} is already a source. Choose repositories from its existing profile."
            );
            cx.notify();
            return;
        }

        let project_id = project.id;
        let auth_token = if provider == project.provider.as_str() {
            match project_auth_token(project, &self.secret_store) {
                Ok(token) => token,
                Err(error) => {
                    self.message = error;
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };

        let input_for_fetch = raw.clone();
        let provider_for_fetch = default_provider.to_string();
        let auth_token_clone = auth_token.clone();

        self.source_busy = true;
        self.message = format!("Fetching live repositories for @{owner} from {provider}...");
        cx.notify();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = release_desk_store::source_fetcher::fetch_source_input(
                &input_for_fetch,
                &provider_for_fetch,
                auth_token_clone.as_deref(),
            );
            if let Ok((profile, _)) = &res {
                if let Some((url, path)) =
                    WorkspaceSource::from_fetched(profile.clone(), None).avatar_cache_request()
                {
                    let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path);
                }
            }
            let _ = tx.send(res);
        });

        let display_owner = owner.clone();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;

                match rx.try_recv() {
                    Ok(res) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.source_busy = false;
                            match res {
                                Ok((profile, specific_repo)) => {
                                    let source = WorkspaceSource::from_fetched(
                                        profile,
                                        specific_repo.as_deref(),
                                    );
                                    if specific_repo.is_some() && !source.repositories.iter().any(|repo| repo.selected) {
                                        this.message = format!("Repository {} was not found or is not accessible.", specific_repo.as_deref().unwrap_or_default());
                                        cx.notify();
                                        return;
                                    }
                                    let repo_count = source.repositories.len();
                                    let handle = source.handle.clone();
                                    let mut updated = this.settings.clone();
                                    if let Some(project) = updated.projects.iter_mut().find(|p| p.id == project_id) {
                                        if !project.sources.iter().any(|existing| existing.provider == source.provider && existing.handle.eq_ignore_ascii_case(&source.handle)) {
                                            project.sources.push(source);
                                            let source_index = project.sources.len() - 1;
                                            match settings::save(&updated) {
                                                Ok(()) => {
                                                    this.projects = updated.projects.clone();
                                                    this.settings = updated;
                                                    this.workspace_revision = this.workspace_revision.wrapping_add(1);
                                                    this.git_statuses.clear();
                                                    this.release_state = Default::default();
                                                    if this.selected_project_id == Some(project_id) {
                                                        this.source_dialog = views::project_sources::SourceDialog::Select(source_index);
                                                    }
                                                    if this.selected_project_id == Some(project_id) && this.source_input.read(cx).value().trim() == raw {
                                                        this.source_input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
                                                    }
                                                    this.message = format!("Discovered {repo_count} repositories for @{handle}.");
                                                }
                                                Err(error) => this.message = error,
                                            }
                                        } else { this.message = format!("Source @{handle} is already in this project."); }
                                    } else { this.message = "The project was removed while the source was loading.".into(); }
                                }
                                Err(err) => {
                                    this.message =
                                        format!("Could not fetch source @{display_owner}: {err}");
                                }
                            }
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _window, cx| {
                            this.source_busy = false;
                            this.message = format!(
                                "Failed to fetch source @{display_owner}: request aborted."
                            );
                            cx.notify();
                        });
                        return;
                    }
                }
            }


        })
        .detach();
    }

    fn refresh_source(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
        else {
            return;
        };
        let Some(source) = project.sources.get(index) else {
            return;
        };
        let project_id = project.id;
        let handle = source.handle.clone();
        let provider = source.provider.clone();
        let token = if provider == project.provider.as_str() {
            match project_auth_token(project, &self.secret_store) {
                Ok(token) => token,
                Err(error) => {
                    self.message = error;
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };
        let fetch_handle = handle.clone();
        let fetch_provider = provider.clone();
        self.source_busy = true;
        self.message = format!("Refreshing @{handle}…");
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = release_desk_store::source_fetcher::fetch_profile_and_repos(
                &fetch_handle,
                &fetch_provider,
                token.as_deref(),
            );
            if let Ok(profile) = &result {
                if let Some((url, path)) =
                    WorkspaceSource::from_fetched(profile.clone(), None).avatar_cache_request()
                {
                    let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path);
                }
            }
            let _ = tx.send(result);
        });
        cx.spawn_in(window, async move |this, cx| { loop {
            cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
            match rx.try_recv() {
                Ok(result) => { let _ = this.update_in(cx, |this, _, cx| {
                    this.source_busy = false;
                    match result {
                        Err(error) => this.message = format!("Could not refresh @{handle}: {error}"),
                        Ok(profile) => {
                            let mut updated = this.settings.clone();
                            if let Some(source) = updated.projects.iter_mut().find(|p| p.id == project_id).and_then(|p| p.sources.iter_mut().find(|s| s.provider == provider && s.handle.eq_ignore_ascii_case(&handle))) {
                                source.refresh_from_fetched(profile);
                                match settings::save(&updated) {
                                    Ok(()) => { this.settings = updated; this.projects = this.settings.projects.clone(); this.workspace_revision = this.workspace_revision.wrapping_add(1); this.git_statuses.clear(); this.release_state = Default::default(); this.message = format!("Refreshed @{handle}; your selections were kept."); }
                                    Err(error) => this.message = error,
                                }
                            } else { this.message = format!("@{handle} was removed while refreshing."); }
                        }
                    } cx.notify();
                }); return; }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { let _ = this.update_in(cx, |this, _, cx| { this.source_busy = false; this.message = "Source refresh stopped unexpectedly.".into(); cx.notify(); }); return; }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }}).detach();
    }

    fn remove_source(&mut self, source_index: usize, cx: &mut Context<Self>) {
        self.source_dialog = views::project_sources::SourceDialog::Closed;
        self.change_selected_project(
            |project| {
                if source_index < project.sources.len() {
                    project.sources.remove(source_index);
                }
            },
            cx,
        );
    }

    fn open_source_url(&mut self, url: &str, cx: &mut Context<Self>) {
        let safe = url::Url::parse(url).is_ok_and(|parsed| {
            (parsed.scheme() == "https" || parsed.scheme() == "http")
                && parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none()
        });
        if !safe {
            self.message = "Could not open this provider link safely.".into();
            cx.notify();
            return;
        }
        self.message = format!("Opening {url} in browser…");
        cx.open_url(url);
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(url).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", url])
                .spawn();
        }
        cx.notify();
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

    fn refresh_releases(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.release_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
        else {
            return;
        };
        let project_id = project.id;
        let revision = self.workspace_revision;
        let token = match project_auth_token(project, &self.secret_store) {
            Ok(token) => token,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        let jobs: Vec<_> = project
            .sources
            .iter()
            .flat_map(|source| {
                source
                    .repositories
                    .iter()
                    .filter(|r| r.selected)
                    .map(|repo| {
                        (
                            source.provider.clone(),
                            repo_manager::repository_git::repository_owner(source, repo)
                                .to_string(),
                            repo.name.clone(),
                            if source.provider == project.provider.as_str() {
                                token.clone()
                            } else {
                                None
                            },
                        )
                    })
            })
            .collect();
        self.release_busy = true;
        self.release_state = repo_manager::workspace_releases::ReleaseViewState::Loading;
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let results = jobs
                .into_iter()
                .map(|(provider, owner, name, token)| {
                    let result = release_desk_store::workspace_releases::fetch_releases(
                        &provider, &owner, &name, token,
                    );
                    repo_manager::workspace_releases::RepositoryReleases {
                        provider,
                        owner,
                        repository: name,
                        result,
                    }
                })
                .collect();
            let _ = tx.send(results);
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(results) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_busy = false;
                            if this.selected_project_id == Some(project_id)
                                && this.workspace_revision == revision
                            {
                                this.release_state =
                                    repo_manager::workspace_releases::ReleaseViewState::Loaded(
                                        results,
                                    );
                            }
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_busy = false;
                            this.release_state = Default::default();
                            this.message = "Release fetch stopped unexpectedly.".into();
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn download_release(
        &mut self,
        provider: String,
        owner: String,
        repo: String,
        tag: String,
        name: String,
        url: String,
        size: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.release_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
        else {
            return;
        };
        let directory = project.directory.clone();
        let private = project.sources.iter().any(|source| {
            source.provider == provider
                && source.repositories.iter().any(|item| {
                    item.name == repo
                        && item.is_private
                        && repo_manager::repository_git::repository_owner(source, item)
                            .eq_ignore_ascii_case(&owner)
                })
        });
        let token = if private && project.provider.as_str() == provider {
            match project_auth_token(project, &self.secret_store) {
                Ok(token) => token,
                Err(error) => {
                    self.message = error;
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };
        if private && token.is_none() {
            self.message =
                format!("Connect a {provider} account with access to download this private asset.");
            cx.notify();
            return;
        }
        self.release_busy = true;
        self.message = format!("Downloading {name}…");
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = match (provider.as_str(), token) {
                ("github", Some(token)) => {
                    release_desk_store::workspace_releases::download_github_asset(
                        &directory, &owner, &repo, &tag, &name, size, &token,
                    )
                }
                ("gitlab", Some(token)) => {
                    release_desk_store::workspace_releases::download_gitlab_asset(
                        &directory, &owner, &repo, &tag, &name, &url, size, &token,
                    )
                }
                (_, Some(_)) => {
                    Err("Private downloads are not supported for this provider.".into())
                }
                (_, None) => release_desk_store::workspace_releases::download_asset(
                    &directory, &owner, &repo, &tag, &name, &url, size,
                ),
            };
            let _ = tx.send(result);
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_busy = false;
                            this.message = match result {
                                Ok(path) => format!("Downloaded to {}", path.display()),
                                Err(error) => format!("Download failed: {error}"),
                            };
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.release_busy = false;
                            this.message = "Download stopped unexpectedly.".into();
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }

    fn check_repository_updates(
        &mut self,
        update: Option<(usize, usize)>,
        local_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
        else {
            return;
        };
        let project_id = project.id;
        let revision = self.workspace_revision;
        let targets = match repo_manager::repository_git::targets(project) {
            Ok(targets) => targets,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        self.git_busy = true;
        self.message = if local_only {
            "Inspecting local repositories…"
        } else if update.is_some() {
            "Updating repository…"
        } else {
            "Checking repositories for updates…"
        }
        .into();
        cx.notify();
        let previous_statuses = self.git_statuses.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut message = None;
            let statuses = targets
                .iter()
                .map(|target| {
                    if update == Some((target.source_index, target.repo_index)) {
                        message = Some(match repo_manager::repository_git::update(target) {
                            Ok(outcome) => format!(
                                "{}: {}",
                                target.name,
                                match outcome {
                                    release_desk_git::SyncOutcome::FastForwarded { .. } =>
                                        "updated successfully".to_string(),
                                    release_desk_git::SyncOutcome::AlreadyCurrent =>
                                        "already up to date".to_string(),
                                    release_desk_git::SyncOutcome::Skipped(state) =>
                                        format!("not updated ({:?})", state.status),
                                    release_desk_git::SyncOutcome::Cloned { .. } =>
                                        "cloned".to_string(),
                                }
                            ),
                            Err(error) => format!("{}: {error}", target.name),
                        });
                    }
                    if local_only {
                        repo_manager::repository_git::inspect(target)
                    } else if update.is_none()
                        || update == Some((target.source_index, target.repo_index))
                    {
                        repo_manager::repository_git::check_for_updates(target)
                    } else {
                        previous_statuses
                            .iter()
                            .find(|status| {
                                status.source_index == target.source_index
                                    && status.repo_index == target.repo_index
                            })
                            .cloned()
                            .unwrap_or_else(|| repo_manager::repository_git::inspect(target))
                    }
                })
                .collect::<Vec<_>>();
            let _ = tx.send((statuses, message));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
                match rx.try_recv() {
                    Ok((statuses, message)) => { let _ = this.update_in(cx, |this, _, cx| {
                        this.git_busy = false;
                        if this.selected_project_id == Some(project_id) && this.workspace_revision == revision {
                            this.git_statuses = statuses;
                            this.message = message.unwrap_or_else(|| if local_only { "Local repository inspection finished." } else { "Repository check finished. See each repository for its status." }.into());
                        }
                        cx.notify();
                    }); return; }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => { let _ = this.update_in(cx, |this, _, cx| { this.git_busy = false; this.message = "Repository check stopped unexpectedly.".into(); cx.notify(); }); return; }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        }).detach();
    }

    fn execute_clone_plan(
        &mut self,
        source_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git_busy {
            return;
        }
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let targets = match repo_manager::repository_git::targets(&project) {
            Ok(targets) if !targets.is_empty() => targets
                .into_iter()
                .filter(|target| target.source_index == source_index && !target.path.exists())
                .collect::<Vec<_>>(),
            Ok(_) => {
                self.message = "Select at least one repository first.".into();
                cx.notify();
                return;
            }
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };
        if targets.is_empty() {
            self.message = "That repository is no longer selected.".into();
            cx.notify();
            return;
        }
        let project_id = project.id;
        let revision = self.workspace_revision;
        self.source_dialog = views::project_sources::SourceDialog::Closed;
        self.project_section = ProjectSection::Repositories;
        self.git_busy = true;
        self.git_statuses.clear();
        self.message = format!("Cloning {} repositories…", targets.len());
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut results = Vec::new();
            for (index, target) in targets.iter().enumerate() {
                let _ = tx.send((
                    false,
                    format!(
                        "Cloning {} of {}: {}…",
                        index + 1,
                        targets.len(),
                        target.name
                    ),
                ));
                let result = repo_manager::repository_git::clone_selected(
                    &project,
                    target.source_index,
                    target.repo_index,
                );
                results.push(format!(
                    "{}: {}",
                    target.name,
                    match result {
                        Ok(release_desk_git::SyncOutcome::Cloned { .. }) => "cloned".to_string(),
                        Ok(release_desk_git::SyncOutcome::Skipped(state)) =>
                            format!("clone needs attention ({:?})", state.status),
                        Ok(_) => "clone completed with an unexpected outcome".to_string(),
                        Err(error) => format!("not cloned: {error}"),
                    }
                ));
            }
            let _ = tx.send((true, results.join(" · ")));
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok((finished, message)) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            if finished {
                                this.git_busy = false;
                            }
                            if this.selected_project_id == Some(project_id)
                                && this.workspace_revision == revision
                            {
                                if finished {
                                    this.git_statuses.clear();
                                }
                                this.message = message;
                            }
                            cx.notify();
                        });
                        if finished {
                            return;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            this.git_busy = false;
                            this.message = "Clone operation stopped unexpectedly.".into();
                            cx.notify();
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
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
                        device_user_code: self.device_flow_user_code.clone(),
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
                        on_copy_code: |this: &mut Self, code, cx| {
                            this.copy_user_code(code, cx);
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
                        ProjectSection::Sources => Some(
                            views::render_project_sources(
                                &project.sources,
                                &project.directory,
                                project.disk_layout,
                                &self.source_input,
                                &self.source_search,
                                self.source_dialog,
                                self.source_busy,
                                cx,
                                |this: &mut Self, window, cx| this.add_source(window, cx),
                                |index, this: &mut Self, cx| this.remove_source(index, cx),
                                |index, this: &mut Self, window, cx| {
                                    this.refresh_source(index, window, cx)
                                },
                                |source, repo, this: &mut Self, cx| {
                                    this.toggle_source_repo(source, repo, cx)
                                },
                                |source, this: &mut Self, window, cx| {
                                    this.execute_clone_plan(source, window, cx)
                                },
                                |dialog, this: &mut Self, cx| {
                                    this.source_dialog = dialog;
                                    cx.notify();
                                },
                                |url, this: &mut Self, cx| this.open_source_url(&url, cx),
                            )
                            .into_any_element(),
                        ),
                        ProjectSection::Repositories => Some(
                            views::render_project_repository_status(
                                project,
                                &self.git_statuses,
                                self.selected_repository,
                                self.git_busy,
                                cx,
                                |this: &mut Self, window, cx| {
                                    this.check_repository_updates(None, false, window, cx)
                                },
                                |source, repo, this: &mut Self, window, cx| {
                                    this.check_repository_updates(
                                        Some((source, repo)),
                                        false,
                                        window,
                                        cx,
                                    )
                                },
                                |source, repo, this: &mut Self, cx| {
                                    this.selected_repository = Some((source, repo));
                                    cx.notify();
                                },
                                |url, this: &mut Self, cx| this.open_source_url(&url, cx),
                            )
                            .into_any_element(),
                        ),
                        ProjectSection::Releases => Some(
                            views::render_live_releases(
                                &self.release_state,
                                &project.directory,
                                self.release_busy,
                                cx,
                                |this: &mut Self, window, cx| this.refresh_releases(window, cx),
                                |provider,
                                 owner,
                                 repo,
                                 tag,
                                 name,
                                 url,
                                 size,
                                 this: &mut Self,
                                 window,
                                 cx| {
                                    this.download_release(
                                        provider, owner, repo, tag, name, url, size, window, cx,
                                    )
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
                        |section, this: &mut Self, window, cx| {
                            this.project_section = section;
                            this.delete_confirm = false;
                            if section == ProjectSection::Repositories
                                && this.git_statuses.is_empty()
                            {
                                this.check_repository_updates(None, true, window, cx);
                            }
                            if section == ProjectSection::Releases
                                && matches!(
                                    this.release_state,
                                    repo_manager::workspace_releases::ReleaseViewState::NotLoaded
                                )
                            {
                                this.refresh_releases(window, cx);
                            }
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
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
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
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .size_full()
                            .overflow_hidden()
                            .child(content),
                    ),
            )
    }
}

fn main() {
    if let Some(code) = repo_manager::git_credential_helper::dispatch() {
        std::process::exit(code);
    }
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
