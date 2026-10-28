//! Release Desk main application state, shell orchestration, and layout rendering.

use crate::colors::hsla;
use crate::components::{render_header, render_sidebar};
use crate::settings::{self, Appearance, Settings, StartView};
use crate::state::WorkspaceTab;
use crate::views;
use crate::workspace::{
    AuthModeChoice, ProjectSection, ProviderChoice, WorkspaceProject,
    find_saved_accounts_for_provider,
};
use crate::workspace_sources::WorkspaceSource;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::InputState;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window,
    div,
};
use release_desk_application::event::EventBroadcaster;
use release_desk_application::use_case::auth::AuthenticateProviderUseCase;
use release_desk_core::app_catalog::AppStoreItem;
use release_desk_core::port::SecretStorePort as _;
use release_desk_core::project::DiskLayout;
use release_desk_store::auth::{LocalOAuthServer, ModalWebviewAuthPort, SystemBrowserAuthPort};
use release_desk_store::credentials::OsSecretStore;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NavLocation {
    ProjectHub,
    Project(u128, ProjectSection),
    AppStore,
    AppDetail(String),
    Settings,
}

pub struct ReleaseDesk {
    pub(crate) settings: Settings,
    pub(crate) projects: Vec<WorkspaceProject>,
    pub(crate) tab: WorkspaceTab,
    pub(crate) selected_project_id: Option<u128>,
    pub(crate) project_section: ProjectSection,
    pub(crate) delete_confirm: bool,
    pub(crate) setup_active: bool,
    pub(crate) setup_step: usize,
    pub(crate) editing_project_id: Option<u128>,
    pub(crate) setup_provider: ProviderChoice,
    pub(crate) setup_layout: DiskLayout,
    pub(crate) name_input: Entity<InputState>,
    pub(crate) directory_input: Entity<InputState>,
    pub(crate) account_input: Entity<InputState>,
    pub(crate) custom_host_input: Entity<InputState>,
    pub(crate) oauth_client_id_input: Entity<InputState>,
    pub(crate) token_input: Entity<InputState>,
    pub(crate) source_input: Entity<InputState>,
    pub(crate) source_search: Entity<InputState>,
    pub(crate) repo_search: Entity<InputState>,
    pub(crate) source_dialog: views::project_sources::SourceDialog,
    pub(crate) source_busy: bool,
    pub(crate) store_items: Vec<AppStoreItem>,
    pub(crate) selected_app_id: Option<String>,
    pub(crate) git_statuses: Vec<crate::repository_git::RepositoryStatus>,
    pub(crate) git_check_completed: usize,
    pub(crate) git_check_total: usize,
    pub(crate) git_activity: Option<crate::controllers::git_controller::GitActivity>,
    pub(crate) git_pending: std::collections::HashSet<(usize, usize)>,
    pub(crate) repository_presentations: std::collections::HashMap<
        (usize, usize),
        views::project_repository_status::RepositoryPresentation,
    >,
    pub(crate) selected_repository: Option<(usize, usize)>,
    pub(crate) languages_expanded: std::collections::HashSet<(usize, usize)>,
    pub(crate) language_breakdowns: std::collections::HashMap<
        (usize, usize),
        views::project_repository_status::LanguageBreakdownState,
    >,
    pub(crate) commits_collapsed: bool,
    pub(crate) selected_editor_id: String,
    pub(crate) editor_menu_open: bool,
    pub(crate) git_busy: bool,
    pub(crate) workspace_revision: u64,
    pub(crate) release_state: crate::workspace_releases::ReleaseViewState,
    pub(crate) release_busy: bool,
    pub(crate) release_type_busy: bool,
    pub(crate) release_type_menu_open: bool,
    pub(crate) message: String,
    last_notified_message: String,
    pub(crate) nav_history: Vec<NavLocation>,
    pub(crate) nav_index: usize,
    pub(crate) secret_store: Arc<OsSecretStore>,
    #[allow(dead_code)]
    pub(crate) auth_uc: AuthenticateProviderUseCase,
    pub(crate) selected_saved_account: Option<String>,
    pub(crate) saved_credential_ref: Option<String>,
    pub(crate) auth_mode_choice: AuthModeChoice,
    pub(crate) auth_status_message: Option<String>,
    pub(crate) detected_system_credential: Option<(String, String)>,
    pub(crate) local_oauth_server: Option<LocalOAuthServer>,
    pub(crate) device_flow_user_code: Option<String>,
    pub(crate) device_flow_verification_uri: Option<String>,
    pub(crate) device_flow_canceller: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) active_webview_child: Option<std::process::Child>,
    pub(crate) last_browser_open: Option<(String, std::time::Instant)>,
    pub(crate) import_dialog: Option<views::import_dialog::ImportDialogState>,
}

pub(crate) fn project_auth_token(
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

pub(crate) fn find_webview_binary() -> Option<std::path::PathBuf> {
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
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let repo_search = cx.new(|cx| InputState::new(window, cx).placeholder("Find a repository"));
        cx.observe(&repo_search, |_, _, cx| cx.notify()).detach();
        let secret_store = Arc::new(OsSecretStore::new());
        let browser_auth = Arc::new(SystemBrowserAuthPort::new());
        let webview_auth = Arc::new(ModalWebviewAuthPort::new());
        let auth_uc = AuthenticateProviderUseCase::new(
            secret_store.clone(),
            browser_auth.clone(),
            webview_auth.clone(),
            EventBroadcaster::default(),
        );
        let selected_editor_id =
            if views::project_repository_status::is_app_installed("Visual Studio Code") {
                "vscode".to_string()
            } else if views::project_repository_status::is_app_installed("Cursor") {
                "cursor".to_string()
            } else if views::project_repository_status::is_app_installed("Xcode") {
                "xcode".to_string()
            } else if views::project_repository_status::is_app_installed("Android Studio") {
                "android-studio".to_string()
            } else {
                "finder".to_string()
            };
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
            repo_search,
            source_dialog: Default::default(),
            source_busy: false,
            store_items: Vec::new(),
            selected_app_id: None,
            git_statuses: Vec::new(),
            git_check_completed: 0,
            git_check_total: 0,
            git_activity: None,
            git_pending: Default::default(),
            repository_presentations: Default::default(),
            selected_repository: None,
            languages_expanded: Default::default(),
            language_breakdowns: Default::default(),
            commits_collapsed: false,
            selected_editor_id,
            editor_menu_open: false,
            git_busy: false,
            workspace_revision: 0,
            release_state: Default::default(),
            release_busy: false,
            release_type_busy: false,
            release_type_menu_open: false,
            message,
            last_notified_message: String::new(),
            nav_history: vec![initial_loc],
            nav_index: 0,
            secret_store,
            auth_uc,
            selected_saved_account: None,
            saved_credential_ref: None,
            auth_mode_choice: AuthModeChoice::OAuthBrowser,
            auth_status_message: None,
            detected_system_credential: None,
            local_oauth_server: None,
            device_flow_user_code: None,
            device_flow_verification_uri: None,
            device_flow_canceller: None,
            active_webview_child: None,
            last_browser_open: None,
            import_dialog: None,
        };
        app.apply_appearance(window, cx);
        let mut download_requests: Vec<(String, std::path::PathBuf)> = app
            .projects
            .iter()
            .flat_map(|project| &project.sources)
            .filter_map(WorkspaceSource::avatar_cache_request)
            .filter(|(_, path)| !path.is_file())
            .collect();

        for project in &app.projects {
            for source in &project.sources {
                for repo in &source.repositories {
                    let repo_dir = crate::repository_git::resolve_target_path(
                        &project.directory,
                        project.disk_layout,
                        crate::repository_git::repository_owner(source, repo),
                        &repo.name,
                    );
                    if crate::components::app_icon::find_local_repo_icon(&repo_dir, &repo.name)
                        .is_none()
                    {
                        if let Some((url, path)) =
                            crate::components::app_icon::repo_icon_cache_request(
                                &repo.name,
                                Some(crate::repository_git::repository_owner(source, repo)),
                            )
                        {
                            if !path.is_file() {
                                download_requests.push((url, path));
                            }
                        }
                    }
                }
            }
        }

        if !download_requests.is_empty() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for (url, path) in download_requests {
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

    pub(crate) fn navigate_to(&mut self, loc: NavLocation, cx: &mut Context<Self>) {
        if self.nav_index < self.nav_history.len() && self.nav_history[self.nav_index] == loc {
            return;
        }
        if self.nav_index + 1 < self.nav_history.len() {
            self.nav_history.truncate(self.nav_index + 1);
        }
        self.nav_history.push(loc.clone());
        self.nav_index = self.nav_history.len() - 1;
        self.apply_nav_location(loc, cx);
        cx.notify();
    }

    pub(crate) fn apply_nav_location(&mut self, loc: NavLocation, cx: &mut Context<Self>) {
        let next_project = match &loc {
            NavLocation::Project(id, _) => Some(*id),
            _ => None,
        };
        let project_changed = next_project != self.selected_project_id;
        if project_changed {
            self.workspace_revision = self.workspace_revision.wrapping_add(1);
            self.git_statuses.clear();
            self.git_check_completed = 0;
            self.git_check_total = 0;
            self.git_activity = None;
            self.git_pending.clear();
            self.git_busy = false;
            self.repository_presentations.clear();
            self.selected_repository = None;
            self.languages_expanded.clear();
            self.language_breakdowns.clear();
            self.release_state = Default::default();
            self.release_busy = false;
            self.release_type_busy = false;
            self.release_type_menu_open = false;
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
        if project_changed {
            if let Some(project_id) = self.selected_project_id {
                self.prefetch_project_data(project_id, cx);
            }
        }
    }

    pub(crate) fn prefetch_project_data(&mut self, project_id: u128, cx: &mut Context<Self>) {
        if self.selected_project_id != Some(project_id) {
            return;
        }
        if !self.git_busy {
            self.check_repository_updates(None, true, cx);
        }
        if !self.release_busy {
            self.refresh_releases(cx);
        }
    }

    pub(crate) fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.nav_index > 0 {
            self.nav_index -= 1;
            let loc = self.nav_history[self.nav_index].clone();
            self.apply_nav_location(loc, cx);
            cx.notify();
        }
    }

    pub(crate) fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.nav_index + 1 < self.nav_history.len() {
            self.nav_index += 1;
            let loc = self.nav_history[self.nav_index].clone();
            self.apply_nav_location(loc, cx);
            cx.notify();
        }
    }

    pub(crate) fn apply_appearance(&self, window: &Window, cx: &mut Context<Self>) {
        let mode = match self.settings.appearance {
            Appearance::System => ThemeMode::from(window.appearance()),
            Appearance::Light => ThemeMode::Light,
            Appearance::Dark => ThemeMode::Dark,
        };
        Theme::change(mode, None, cx);
        let is_dark = mode.is_dark();
        Theme::update(cx, |theme| {
            if is_dark {
                theme.background = hsla(216.0 / 360.0, 0.23, 0.086, 1.0);
                theme.secondary = hsla(219.0 / 360.0, 0.21, 0.13, 1.0);
                theme.border = hsla(218.0 / 360.0, 0.20, 0.22, 1.0);
                theme.primary = hsla(232.0 / 360.0, 1.0, 0.78, 1.0);
                theme.primary_foreground = hsla(225.0 / 360.0, 0.32, 0.09, 1.0);
                theme.foreground = hsla(215.0 / 360.0, 0.22, 0.93, 1.0);
                theme.muted_foreground = hsla(216.0 / 360.0, 0.17, 0.65, 1.0);
                theme.muted = hsla(229.0 / 360.0, 0.35, 0.21, 1.0);
                theme.success = hsla(150.0 / 360.0, 0.58, 0.60, 1.0);
                theme.warning = hsla(38.0 / 360.0, 0.86, 0.62, 1.0);
                theme.danger = hsla(5.0 / 360.0, 1.0, 0.75, 1.0);
            } else {
                theme.background = hsla(216.0 / 360.0, 0.23, 0.96, 1.0);
                theme.secondary = hsla(0.0, 0.0, 1.0, 1.0);
                theme.border = hsla(218.0 / 360.0, 0.20, 0.87, 1.0);
                theme.primary = hsla(232.0 / 360.0, 0.72, 0.52, 1.0);
                theme.primary_foreground = hsla(0.0, 0.0, 1.0, 1.0);
                theme.foreground = hsla(218.0 / 360.0, 0.28, 0.12, 1.0);
                theme.muted_foreground = hsla(216.0 / 360.0, 0.14, 0.40, 1.0);
                theme.muted = hsla(232.0 / 360.0, 0.73, 0.94, 1.0);
                theme.success = hsla(151.0 / 360.0, 0.68, 0.28, 1.0);
                theme.warning = hsla(35.0 / 360.0, 1.0, 0.30, 1.0);
                theme.danger = hsla(5.0 / 360.0, 0.71, 0.40, 1.0);
            }
            theme.button = theme.secondary;
            theme.button_foreground = theme.foreground;
            theme.button_primary = theme.primary;
            theme.button_primary_foreground = theme.primary_foreground;
            theme.button_primary_hover =
                hsla(232.0 / 360.0, 0.90, if is_dark { 0.83 } else { 0.47 }, 1.0);
            theme.button_primary_active =
                hsla(232.0 / 360.0, 0.90, if is_dark { 0.70 } else { 0.40 }, 1.0);
            theme.tokens.button_primary = theme.button_primary.into();
            theme.tokens.button_primary_hover = theme.button_primary_hover.into();
            theme.tokens.button_primary_active = theme.button_primary_active.into();
        });
    }

    pub(crate) fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.appearance = appearance;
        let _ = settings::save(&self.settings);
        self.apply_appearance(window, cx);
        self.message = format!("Theme changed to {:?}", appearance);
        cx.notify();
    }
}

impl Render for ReleaseDesk {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.message != self.last_notified_message {
            self.last_notified_message.clone_from(&self.message);
            let message = self.message.trim();
            if !message.is_empty()
                && !message.ends_with('…')
                && !message.ends_with("...")
                && message != "Local repository inspection finished."
            {
                let lower = message.to_ascii_lowercase();
                let notification = if lower.contains("failed")
                    || lower.contains("error")
                    || lower.starts_with("cannot ")
                    || lower.starts_with("could not ")
                {
                    Notification::error(message.to_string())
                } else {
                    Notification::info(message.to_string())
                };
                cx.spawn_in(window, async move |_, cx| {
                    let _ = cx.update(|window, cx| window.push_notification(notification, cx));
                })
                .detach();
            }
        }
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
                                &self.repository_presentations,
                                self.selected_repository,
                                &self.repo_search,
                                self.git_busy,
                                self.git_check_completed,
                                self.git_check_total,
                                self.git_activity,
                                &self.git_pending,
                                self.commits_collapsed,
                                &self.languages_expanded,
                                &self.language_breakdowns,
                                &self.selected_editor_id,
                                self.editor_menu_open,
                                cx,
                                |this: &mut Self, _window, cx| {
                                    this.check_repository_updates(None, false, cx)
                                },
                                |source, repo, this: &mut Self, _window, cx| {
                                    this.check_repository_updates(Some((source, repo)), false, cx)
                                },
                                |this: &mut Self, _window, cx| this.pull_all_updates(cx),
                                |source, repo, this: &mut Self, window, cx| {
                                    this.clone_single_repository(source, repo, window, cx)
                                },
                                |this: &mut Self, window, cx| {
                                    this.clone_all_missing_repositories(window, cx)
                                },
                                |source, repo, this: &mut Self, cx| {
                                    this.selected_repository = Some((source, repo));
                                    this.editor_menu_open = false;
                                    cx.notify();
                                },
                                |url, this: &mut Self, cx| this.open_source_url(&url, cx),
                                |this: &mut Self, _window, cx| {
                                    this.project_section = ProjectSection::Sources;
                                    cx.notify();
                                },
                                |this: &mut Self, cx| {
                                    this.commits_collapsed = !this.commits_collapsed;
                                    cx.notify();
                                },
                                |source, repo, this: &mut Self, window, cx| {
                                    this.toggle_repository_languages(source, repo, window, cx)
                                },
                                |this: &mut Self, cx| {
                                    this.editor_menu_open = !this.editor_menu_open;
                                    cx.notify();
                                },
                                |editor_id, this: &mut Self, cx| {
                                    this.selected_editor_id = editor_id;
                                    this.editor_menu_open = false;
                                    cx.notify();
                                },
                                |path, editor_id, this: &mut Self, cx| {
                                    this.editor_menu_open = false;
                                    views::project_repository_status::open_path_in_editor(
                                        &path, &editor_id,
                                    );
                                    cx.notify();
                                },
                            )
                            .into_any_element(),
                        ),
                        ProjectSection::Releases => Some(
                            views::render_live_releases(
                                &self.release_state,
                                views::ReleaseShelfControls {
                                    busy: self.release_busy || self.release_type_busy,
                                    extension: &self.settings.installer_extension,
                                    type_menu_open: self.release_type_menu_open,
                                },
                                cx,
                                |this: &mut Self, _window, cx| this.refresh_releases(cx),
                                |this: &mut Self, cx| {
                                    this.release_type_menu_open = !this.release_type_menu_open;
                                    cx.notify();
                                },
                                |extension, this: &mut Self, cx| {
                                    this.select_release_type(extension, cx)
                                },
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
                        |section, this: &mut Self, _window, cx| {
                            this.project_section = section;
                            this.delete_confirm = false;
                            if section != ProjectSection::Releases {
                                this.release_type_menu_open = false;
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
                        |this: &mut Self, window, cx| this.import_local_repositories(window, cx),
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

        let mut root = div()
            .flex()
            .size_full()
            .relative()
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
                    .child(render_header(
                        self.nav_index > 0,
                        self.nav_index + 1 < self.nav_history.len(),
                        location_title,
                        project_name,
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
                            .child(content),
                    ),
            );

        if let Some(ref dialog) = self.import_dialog {
            root = root.child(views::render_import_dialog(
                dialog,
                cx,
                |idx, this: &mut Self, _window, cx| this.toggle_import_repo(idx, cx),
                |select, this: &mut Self, _window, cx| this.select_all_import_repos(select, cx),
                |handle, this: &mut Self, window, cx| {
                    this.confirm_import_dialog(handle, window, cx)
                },
                |this: &mut Self, _window, cx| this.cancel_import_dialog(cx),
            ));
        }

        root
    }
}
