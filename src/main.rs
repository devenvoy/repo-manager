//! Release Desk native macOS GPUI desktop application.

use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window,
    WindowOptions, div,
};
use release_desk_core::latest_installer;
use release_desk_core::snapshot::{MismatchReport, Snapshot};
use release_desk_store::project_backup::ProjectSnapshotManager;
use release_desk_store::{Store, StoreError};
use repo_manager::components::{render_header, render_sidebar};
use repo_manager::settings::{self, Appearance, Settings};
use repo_manager::state::WorkspaceTab;
use repo_manager::views;
use std::path::Path;

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

        let tab = match Store::open() {
            Ok(_) => WorkspaceTab::Overview,
            Err(StoreError::NotInitialized) => WorkspaceTab::Welcome,
            Err(error) => {
                eprintln!("Release Desk library: {error}");
                WorkspaceTab::Welcome
            }
        };

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
        };

        if app.tab != WorkspaceTab::Welcome {
            app.reload_snapshot();
        }
        app.apply_appearance(window, cx);
        app
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
                    .child(render_header("ArtCraft", &self.message, cx))
                    .child(match self.tab {
                        WorkspaceTab::Welcome => views::render_welcome(
                            &self.library_input,
                            &self.path_input,
                            cx,
                            |this: &mut Self, cx| this.initialize_library(cx),
                        )
                        .into_any_element(),
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
