//! Release Desk native desktop shell.

use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, PathPromptOptions, Render,
    Styled as _, Window, WindowOptions, div,
};
use release_desk_core::app_catalog::{AppActionState, AppStoreItem};
use release_desk_core::project::DiskLayout;
use repo_manager::components::{render_header, render_sidebar};
use repo_manager::settings::{self, Appearance, Settings, StartView};
use repo_manager::state::WorkspaceTab;
use repo_manager::views;
use repo_manager::workspace::{ProjectSection, ProviderChoice, WorkspaceProject, expand_directory};
use repo_manager::workspace_sources::{RepositoryListMode, WorkspaceSource};

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
    token_input: Entity<InputState>,
    source_input: Entity<InputState>,
    store_items: Vec<AppStoreItem>,
    selected_app_id: Option<String>,
    reviewing_clones: bool,
    selected_platform: &'static str,
    message: String,
}

impl ReleaseDesk {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (settings, message) = match settings::load() {
            Ok(settings) => (settings, String::new()),
            Err(error) => (Settings::default(), error),
        };
        let projects = settings.projects.clone();
        let tab = match settings.start_view {
            StartView::ProjectHub => WorkspaceTab::ProjectHub,
            StartView::AppStore => WorkspaceTab::AppStore,
        };
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. Product Suite"));
        let directory_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/project/directory"));
        let account_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Account or organization"));
        let custom_host_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://git.example.com"));
        let token_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Personal access token"));
        let source_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Handle or profile URL"));
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
            token_input,
            source_input,
            store_items: Vec::new(),
            selected_app_id: None,
            reviewing_clones: false,
            selected_platform: "macOS",
            message,
        };
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

    fn begin_setup(&mut self, editing: Option<u128>, window: &mut Window, cx: &mut Context<Self>) {
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
        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.directory_input
            .update(cx, |input, cx| input.set_value(directory, window, cx));
        self.account_input
            .update(cx, |input, cx| input.set_value(account, window, cx));
        self.custom_host_input
            .update(cx, |input, cx| input.set_value(host, window, cx));
        self.token_input
            .update(cx, |input, cx| input.set_value(String::new(), window, cx));
        cx.notify();
    }

    fn setup_back(&mut self, cx: &mut Context<Self>) {
        if self.setup_step == 0 {
            self.setup_active = false;
            self.editing_project_id = None;
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
        let account = self.account_input.read(cx).value().trim().to_string();
        let host = self.custom_host_input.read(cx).value().trim().to_string();

        if self.setup_step == 0 && (name.is_empty() || !directory.is_dir()) {
            self.message = "Enter a name and choose an existing directory.".into();
            cx.notify();
            return;
        }
        if self.setup_step == 1
            && (account.is_empty()
                || (self.setup_provider == ProviderChoice::Custom && host.is_empty()))
        {
            self.message = "Enter an account and, for Custom Git, a server URL.".into();
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
        let token = self.token_input.read(cx).value().trim().to_string();
        let credential_ref = if token.is_empty() {
            None
        } else {
            Some(format!("token:project-{id}"))
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
        let location = match self.tab {
            WorkspaceTab::AppStore => "App Store".to_string(),
            WorkspaceTab::Settings => "Settings".to_string(),
            _ => self
                .selected_project_id
                .and_then(|id| self.projects.iter().find(|project| project.id == id))
                .map_or_else(
                    || "All projects".to_string(),
                    |project| project.name.clone(),
                ),
        };
        let content = match self.tab {
            WorkspaceTab::AppStore => views::render_store(
                &self.store_items,
                self.selected_app_id.as_deref(),
                cx,
                |id, this: &mut Self, cx| {
                    this.selected_app_id = Some(id);
                    cx.notify();
                },
                |id, this: &mut Self, cx| this.preview_app_action(id, cx),
                |this: &mut Self, cx| {
                    this.selected_app_id = None;
                    cx.notify();
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
            _ if self.setup_active => views::render_project_setup(
                views::ProjectSetupProps {
                    editing: self.editing_project_id.is_some(),
                    step: self.setup_step,
                    provider: self.setup_provider,
                    disk_layout: self.setup_layout,
                    name: &self.name_input,
                    directory: &self.directory_input,
                    account: &self.account_input,
                    custom_host: &self.custom_host_input,
                    token: &self.token_input,
                },
                cx,
                |provider, this: &mut Self, cx| {
                    this.setup_provider = provider;
                    cx.notify();
                },
                |layout, this: &mut Self, cx| {
                    this.setup_layout = layout;
                    cx.notify();
                },
                |this: &mut Self, cx| this.setup_next(cx),
                |this: &mut Self, cx| this.setup_back(cx),
                |this: &mut Self, window, cx| this.browse_directory(window, cx),
            )
            .into_any_element(),
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
                            this.selected_project_id = None;
                            cx.notify();
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
                            this.selected_project_id = Some(id);
                            this.project_section = ProjectSection::Overview;
                            cx.notify();
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
                    this.tab = tab;
                    this.setup_active = false;
                    this.selected_project_id = None;
                    this.selected_app_id = None;
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
                        &location,
                        &self.message,
                        self.settings.appearance,
                        cx,
                        |this: &mut Self, appearance, window, cx| {
                            this.set_appearance(appearance, window, cx);
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
