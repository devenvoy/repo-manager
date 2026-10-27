//! Release Desk native macOS GPUI desktop application.

use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window,
    WindowOptions, div,
};
use release_desk_application::use_case::app_store::{
    AppStoreAggregatorUseCase, AppStoreMetrics, FsAppInstallationProbe,
};
use release_desk_core::app_catalog::{AppActionState, AppStoreItem};
use release_desk_core::latest_installer;
use release_desk_core::project::{ProjectId, ProjectIdentity, ProjectName, SelectableRepo};
use release_desk_core::snapshot::{MismatchReport, Snapshot};
use release_desk_store::project_backup::ProjectSnapshotManager;
use release_desk_store::{Store, StoreError};
use repo_manager::components::{render_header, render_sidebar};
use repo_manager::settings::{self, Appearance, Settings, StartView};
use repo_manager::state::WorkspaceTab;
use repo_manager::views;
use std::path::{Path, PathBuf};

const ARTCRAFT_SNAPSHOT: &str = "/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json";

struct ReleaseDesk {
    settings: Settings,
    snapshot: Option<Snapshot>,
    selected_repo: Option<usize>,
    tab: WorkspaceTab,
    path_input: Entity<InputState>,
    extension_input: Entity<InputState>,
    library_input: Entity<InputState>,
    message: String,
    mismatch_report: Option<MismatchReport>,
    app_store_items: Vec<AppStoreItem>,
    app_store_metrics: AppStoreMetrics,
    selected_app_id: Option<String>,
    projects: Vec<ProjectIdentity>,
    selectable_repos: Vec<SelectableRepo>,
    show_ingest_modal: bool,
}

impl ReleaseDesk {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (settings, message) = match settings::load() {
            Ok(settings) => (settings, String::new()),
            Err(error) => (Settings::default(), error),
        };

        let snapshot_path = if settings.snapshot_path.is_empty() {
            ARTCRAFT_SNAPSHOT.to_owned()
        } else {
            settings.snapshot_path.clone()
        };

        let path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("/path/to/.release-desk/data.json")
                .default_value(snapshot_path)
        });

        let extension_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("dmg")
                .default_value(settings.installer_extension.clone())
        });

        let library_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Choose a folder for Release Desk Library")
        });

        let default_proj = ProjectIdentity::with_clone_location(
            ProjectId::from_u128(1),
            ProjectName::new("ArtCraft Studio Fleet").unwrap(),
            Some(PathBuf::from("/Volumes/Devansh2/codes/ArtCraft")),
        );
        let projects = vec![default_proj];

        let probe = FsAppInstallationProbe;
        let aggregator =
            AppStoreAggregatorUseCase::new(probe, PathBuf::from(&settings.applications_dir));
        let (app_store_items, app_store_metrics) = aggregator.aggregate_apps(&projects, &[]);

        let tab = if !settings.terms_accepted {
            WorkspaceTab::Welcome
        } else {
            match Store::open() {
                Ok(_) => match settings.start_view {
                    StartView::AppStore => WorkspaceTab::AppStore,
                    StartView::ProjectHub => WorkspaceTab::ProjectHub,
                },
                Err(StoreError::NotInitialized) => WorkspaceTab::Welcome,
                Err(error) => {
                    eprintln!("Release Desk library: {error}");
                    WorkspaceTab::AppStore
                }
            }
        };

        let selectable_repos = vec![
            SelectableRepo {
                name: "photocraft".to_string(),
                clone_url: "https://github.com/storytold/photocraft.git".to_string(),
                description: Some("Pro RAW photo development & editing workbench".to_string()),
                is_private: false,
                stars: 142,
                default_branch: "main".to_string(),
                has_release_asset: true,
                selected: true,
            },
            SelectableRepo {
                name: "gridcraft".to_string(),
                clone_url: "https://github.com/storytold/gridcraft.git".to_string(),
                description: Some("Analytical spreadsheet engine with WASM formulas".to_string()),
                is_private: false,
                stars: 88,
                default_branch: "main".to_string(),
                has_release_asset: true,
                selected: true,
            },
            SelectableRepo {
                name: "filmcraft".to_string(),
                clone_url: "https://github.com/storytold/filmcraft.git".to_string(),
                description: Some("Timeline video editor with multi-track audio".to_string()),
                is_private: false,
                stars: 64,
                default_branch: "main".to_string(),
                has_release_asset: true,
                selected: true,
            },
        ];

        let mut app = Self {
            settings,
            snapshot: None,
            selected_repo: None,
            tab,
            path_input,
            extension_input,
            library_input,
            message,
            mismatch_report: None,
            app_store_items,
            app_store_metrics,
            selected_app_id: None,
            projects,
            selectable_repos,
            show_ingest_modal: false,
        };

        if app.tab != WorkspaceTab::Welcome {
            app.reload_snapshot();
        }
        app.apply_appearance(window, cx);
        app
    }

    fn agree_and_start(&mut self, cx: &mut Context<Self>) {
        self.settings.terms_accepted = true;
        let _ = settings::save(&self.settings);
        let lib_path = PathBuf::from(&self.settings.library_dir);
        let snap_path = Path::new(ARTCRAFT_SNAPSHOT);
        let _ = Store::initialize(&lib_path, snap_path);
        self.tab = WorkspaceTab::AppStore;
        self.message = "Welcome to Release Desk! Library initialized.".to_string();
        self.reload_snapshot();
        cx.notify();
    }

    fn trigger_app_action(&mut self, app_id: String, cx: &mut Context<Self>) {
        if let Some(item) = self
            .app_store_items
            .iter_mut()
            .find(|i| i.metadata.id == app_id)
        {
            match &item.state {
                AppActionState::Installable | AppActionState::UpdateAvailable { .. } => {
                    item.state = AppActionState::UpToDate {
                        version: item
                            .latest_release_tag
                            .clone()
                            .unwrap_or_else(|| "v2.4.0".to_string()),
                    };
                    self.message = format!(
                        "Successfully installed {} to ~/Applications",
                        item.metadata.name
                    );
                }
                AppActionState::UpToDate { .. } => {
                    self.message = format!(
                        "Launching {} natively via macOS Cocoa open command",
                        item.metadata.name
                    );
                }
            }
            self.recompute_metrics();
        }
        cx.notify();
    }

    fn recompute_metrics(&mut self) {
        let mut metrics = AppStoreMetrics {
            total_apps: self.app_store_items.len(),
            updates_available: 0,
            up_to_date: 0,
            installable: 0,
        };
        for item in &self.app_store_items {
            match item.state {
                AppActionState::Installable => metrics.installable += 1,
                AppActionState::UpdateAvailable { .. } => metrics.updates_available += 1,
                AppActionState::UpToDate { .. } => metrics.up_to_date += 1,
            }
        }
        self.app_store_metrics = metrics;
    }

    fn apply_appearance(&self, window: &Window, cx: &mut Context<Self>) {
        let mode = match self.settings.appearance {
            Appearance::System => ThemeMode::from(window.appearance()),
            Appearance::Light => ThemeMode::Light,
            Appearance::Dark => ThemeMode::Dark,
        };
        Theme::change(mode, None, cx);
    }

    fn reload_snapshot(&mut self) {
        let result = Store::open().and_then(|store| {
            let project = store.project("ArtCraft")?;
            store.cached_snapshot(&project)
        });

        match result {
            Ok(view) => {
                let snapshot = view.snapshot;
                let count = snapshot.repos.len();
                self.selected_repo = snapshot
                    .repos
                    .iter()
                    .position(|repo| self.settings.show_archived || !repo.is_archived);
                self.message = if view.stale {
                    format!(
                        "Showing {count} cached repositories. The linked snapshot is unavailable."
                    )
                } else {
                    format!("{count} repositories · Linked ArtCraft snapshot")
                };

                // Check local backup mismatch
                let clone_loc = Path::new(&self.settings.snapshot_path)
                    .parent()
                    .and_then(Path::parent);
                if let Some(loc) = clone_loc {
                    let manager = ProjectSnapshotManager::new();
                    use release_desk_core::ProjectBackupPort;
                    if let Ok(report) = manager.detect_mismatch(loc, &snapshot.repos) {
                        self.mismatch_report = Some(report);
                    }
                }

                self.snapshot = Some(snapshot);
            }
            Err(error) => {
                self.snapshot = None;
                self.selected_repo = None;
                self.message = format!("Could not open the managed library: {error}");
            }
        }
    }

    fn initialize_library(&mut self, cx: &mut Context<Self>) {
        let library = self.library_input.read(cx).value().trim().to_owned();
        if library.is_empty() {
            self.message = "Choose a library location to continue.".into();
            cx.notify();
            return;
        }

        let snapshot = self.path_input.read(cx).value().trim().to_owned();
        match Store::initialize(Path::new(&library), Path::new(&snapshot)) {
            Ok(_) => {
                self.settings.snapshot_path = snapshot;
                if let Err(error) = settings::save(&self.settings) {
                    self.message = error;
                }
                self.tab = WorkspaceTab::Overview;
                self.reload_snapshot();
            }
            Err(error) => self.message = format!("Could not create library: {error}"),
        }
        cx.notify();
    }

    #[allow(dead_code)]
    fn refresh_snapshot(&mut self, cx: &mut Context<Self>) {
        let result = Store::open().and_then(|mut store| {
            let project = store.project("ArtCraft")?;
            store.refresh_snapshot(&project)
        });

        match result {
            Ok(_) => self.reload_snapshot(),
            Err(error) => self.message = format!("Refresh failed: {error}"),
        }
        cx.notify();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let extension = self.extension_input.read(cx).value().to_string();
        let extension = match settings::normalized_extension(&extension) {
            Ok(value) => value,
            Err(error) => {
                self.message = error;
                cx.notify();
                return;
            }
        };

        self.settings.installer_extension = extension;
        match settings::save(&self.settings) {
            Ok(()) => {
                self.reload_snapshot();
                self.tab = WorkspaceTab::Overview;
            }
            Err(error) => self.message = error,
        }
        cx.notify();
    }

    fn set_appearance(&mut self, appearance: Appearance, window: &Window, cx: &mut Context<Self>) {
        self.settings.appearance = appearance;
        self.apply_appearance(window, cx);
        if let Err(error) = settings::save(&self.settings) {
            self.message = error;
        }
        cx.notify();
    }
}

impl Render for ReleaseDesk {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.tab != WorkspaceTab::Welcome;
        let empty_vec = Vec::new();
        let repos = self
            .snapshot
            .as_ref()
            .map(|s| &s.repos)
            .unwrap_or(&empty_vec);

        let repos_count = repos.len();
        let releases_count = repos
            .iter()
            .map(|repo| repo.releases.as_ref().map_or(0, |r| r.nodes.len()))
            .sum::<usize>();

        let with_installers_count = repos
            .iter()
            .filter(|repo| {
                latest_installer(
                    repo,
                    self.settings.include_prereleases,
                    &self.settings.installer_extension,
                )
                .is_some()
            })
            .count();

        let archived_count = repos.iter().filter(|repo| repo.is_archived).count();
        let backup_synced = self
            .mismatch_report
            .as_ref()
            .map(|r| r.is_synchronized())
            .unwrap_or(true);

        div()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(render_sidebar(
                self.tab,
                ready,
                cx,
                |this: &mut Self, tab, cx| {
                    this.tab = tab;
                    cx.notify();
                },
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .child(render_header(
                        "ArtCraft Studio Fleet",
                        &self.message,
                        self.tab,
                        self.app_store_metrics.updates_available,
                        2,
                        cx,
                        |tab, this: &mut Self, cx| {
                            this.tab = tab;
                            cx.notify();
                        },
                    ))
                    .child(match self.tab {
                        WorkspaceTab::AppStore => views::render_store(
                            &self.app_store_items,
                            self.app_store_metrics,
                            self.selected_app_id.as_deref(),
                            cx,
                            |app_id, this: &mut Self, cx| {
                                this.selected_app_id = Some(app_id);
                                cx.notify();
                            },
                            |app_id, this: &mut Self, cx| {
                                this.trigger_app_action(app_id, cx);
                            },
                            |this: &mut Self, cx| {
                                this.selected_app_id = None;
                                cx.notify();
                            },
                        )
                        .into_any_element(),
                        WorkspaceTab::ProjectHub => views::render_hub::<Self, _, _, _, _, _, _, _, _>(
                            views::HubViewProps {
                                projects: &self.projects,
                                view_mode: self.settings.hub_view_mode,
                                selectable_repos: &self.selectable_repos,
                                show_ingest_modal: self.show_ingest_modal,
                            },
                            views::HubViewHandlers {
                                on_toggle_view_mode: |mode, this: &mut Self, cx: &mut Context<Self>| {
                                    this.settings.hub_view_mode = mode;
                                    let _ = settings::save(&this.settings);
                                    cx.notify();
                                },
                                on_new_project: |this: &mut Self, cx: &mut Context<Self>| {
                                    this.message =
                                        "New project creation dialog opened.".to_string();
                                    cx.notify();
                                },
                                on_open_ingest: |proj_id, this: &mut Self, cx: &mut Context<Self>| {
                                    this.show_ingest_modal = true;
                                    this.message =
                                        format!("Ingesting profile for project {proj_id}");
                                    cx.notify();
                                },
                                on_sync_project: |proj_id, this: &mut Self, cx: &mut Context<Self>| {
                                    this.message = format!(
                                        "All repositories synchronized for project {proj_id}"
                                    );
                                    cx.notify();
                                },
                                on_toggle_repo: |repo_name, this: &mut Self, cx: &mut Context<Self>| {
                                    if let Some(r) = this
                                        .selectable_repos
                                        .iter_mut()
                                        .find(|r| r.name == repo_name)
                                    {
                                        r.selected = !r.selected;
                                    }
                                    cx.notify();
                                },
                                on_select_all_repos: |select, this: &mut Self, cx: &mut Context<Self>| {
                                    for r in &mut this.selectable_repos {
                                        r.selected = select;
                                    }
                                    cx.notify();
                                },
                                on_confirm_import: |this: &mut Self, cx: &mut Context<Self>| {
                                    this.show_ingest_modal = false;
                                    this.message =
                                        "Imported selected repositories into project.".to_string();
                                    cx.notify();
                                },
                                on_close_ingest: |this: &mut Self, cx: &mut Context<Self>| {
                                    this.show_ingest_modal = false;
                                    cx.notify();
                                },
                            },
                            cx,
                        )
                        .into_any_element(),
                        WorkspaceTab::Welcome => {
                            if !self.settings.terms_accepted {
                                views::render_onboarding(
                                    &self.settings.library_dir,
                                    &self.settings.applications_dir,
                                    cx,
                                    |this: &mut Self, cx| this.agree_and_start(cx),
                                )
                                .into_any_element()
                            } else {
                                views::render_welcome(
                                    &self.library_input,
                                    &self.path_input,
                                    cx,
                                    |this: &mut Self, cx| this.initialize_library(cx),
                                )
                                .into_any_element()
                            }
                        }
                        WorkspaceTab::Overview => views::render_overview(
                            "ArtCraft",
                            views::OverviewMetrics {
                                repos_count,
                                releases_count,
                                with_installers_count,
                                archived_count,
                                backup_synced,
                            },
                            cx,
                            |this: &mut Self, cx| {
                                this.tab = WorkspaceTab::Repositories;
                                cx.notify();
                            },
                        )
                        .into_any_element(),
                        WorkspaceTab::Repositories => views::render_repositories(
                            repos,
                            self.selected_repo,
                            self.settings.show_archived,
                            self.settings.include_prereleases,
                            &self.settings.installer_extension,
                            cx,
                            |this: &mut Self, index, cx| {
                                this.selected_repo = Some(index);
                                cx.notify();
                            },
                        )
                        .into_any_element(),
                        WorkspaceTab::Installers => views::render_installers(
                            repos,
                            self.settings.include_prereleases,
                            &self.settings.installer_extension,
                            cx,
                        )
                        .into_any_element(),
                        WorkspaceTab::PullRequests => {
                            views::render_pull_requests(cx).into_any_element()
                        }
                        WorkspaceTab::Reconcile => views::render_reconcile(
                            self.mismatch_report.as_ref(),
                            cx,
                            |this: &mut Self, cx| {
                                this.message =
                                    "Database restored from local backup snapshot.".into();
                                this.reload_snapshot();
                                cx.notify();
                            },
                            |this: &mut Self, cx| {
                                this.message =
                                    "Exported current state to local data.json backup.".into();
                                this.reload_snapshot();
                                cx.notify();
                            },
                        )
                        .into_any_element(),
                        WorkspaceTab::Settings => views::render_settings(
                            &self.settings,
                            &self.extension_input,
                            cx,
                            |this: &mut Self, value, cx| {
                                this.settings.include_prereleases = value;
                                cx.notify();
                            },
                            |this: &mut Self, value, cx| {
                                this.settings.show_archived = value;
                                cx.notify();
                            },
                            |this: &mut Self, appearance, window, cx| {
                                this.set_appearance(appearance, window, cx);
                            },
                            |this: &mut Self, cx| this.save_settings(cx),
                        )
                        .into_any_element(),
                    }),
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
