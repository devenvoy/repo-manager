mod settings;

use gpui_kit::component::{
    ActiveTheme as _, Theme, ThemeMode,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    form::{Field, Form},
    input::{Input, InputState},
    scroll::ScrollableElement as _,
};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window,
    WindowOptions, base::StyledExt, div, px,
};
use release_desk_core::{Repository, Snapshot, latest_installer};
use release_desk_store::{Store, StoreError};
use settings::{Appearance, Settings};
use std::path::Path;

const ARTCRAFT_SNAPSHOT: &str = "/Volumes/Devansh2/codes/ArtCraft/.release-desk/data.json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome,
    Overview,
    Repositories,
    Installers,
    Settings,
}

struct ReleaseDesk {
    settings: Settings,
    snapshot: Option<Snapshot>,
    selected: Option<usize>,
    page: Page,
    path_input: Entity<InputState>,
    extension_input: Entity<InputState>,
    library_input: Entity<InputState>,
    message: String,
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
        let page = match Store::open() {
            Ok(_) => Page::Overview,
            Err(StoreError::NotInitialized) => Page::Welcome,
            Err(error) => {
                eprintln!("Release Desk library: {error}");
                Page::Welcome
            }
        };
        let mut app = Self {
            settings,
            snapshot: None,
            selected: None,
            page,
            path_input,
            extension_input,
            library_input,
            message,
        };
        if app.page != Page::Welcome {
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
                self.selected = snapshot
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
                self.snapshot = Some(snapshot);
            }
            Err(error) => {
                self.snapshot = None;
                self.selected = None;
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
                self.page = Page::Overview;
                self.reload_snapshot();
            }
            Err(error) => self.message = format!("Could not create library: {error}"),
        }
        cx.notify();
    }

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
                self.page = Page::Overview;
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

    fn welcome_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().gap_4().p_6()
            .child("Your releases, one organized place.")
            .child("Choose where Release Desk should create its managed library. ArtCraft stays where it is and its data.json is linked read-only.")
            .child(Form::new()
                .child(Field::new().label("Managed library parent folder").child(Input::new(&self.library_input)))
                .child(Field::new().label("ArtCraft snapshot").child(Input::new(&self.path_input))))
            .child(Button::new("create-library").primary().label("Create library and open ArtCraft")
                .on_click(cx.listener(|this, _, _, cx| this.initialize_library(cx))))
    }

    fn overview_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (repos, releases, with_installer, archived) =
            self.snapshot.as_ref().map_or((0, 0, 0, 0), |snapshot| {
                (
                    snapshot.repos.len(),
                    snapshot
                        .repos
                        .iter()
                        .map(|repo| repo.releases.nodes.len())
                        .sum::<usize>(),
                    snapshot
                        .repos
                        .iter()
                        .filter(|repo| {
                            latest_installer(
                                repo,
                                self.settings.include_prereleases,
                                &self.settings.installer_extension,
                            )
                            .is_some()
                        })
                        .count(),
                    snapshot
                        .repos
                        .iter()
                        .filter(|repo| repo.is_archived)
                        .count(),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap_5()
            .p_6()
            .overflow_y_scrollbar()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_6()
                    .rounded_lg()
                    .bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
                    .child(
                        div()
                            .text_size(px(28.))
                            .font_bold()
                            .child("ArtCraft overview"),
                    )
                    .child("Release activity and installers from your linked GitHub snapshot."),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(self.metric_card("Repositories", repos, cx))
                    .child(self.metric_card("Releases", releases, cx))
                    .child(self.metric_card("With installers", with_installer, cx))
                    .child(self.metric_card("Archived", archived, cx)),
            )
            .child(
                Button::new("browse-repositories")
                    .primary()
                    .label("Explore repositories")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = Page::Repositories;
                        cx.notify();
                    })),
            )
    }

    fn metric_card(
        &self,
        label: &'static str,
        value: usize,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .flex_1()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().foreground)
            .rounded_lg()
            .child(
                div()
                    .text_size(px(28.))
                    .font_bold()
                    .child(value.to_string()),
            )
            .child(div().text_sm().child(label))
    }

    fn selected_repo(&self) -> Option<&Repository> {
        let index = self.selected?;
        self.snapshot.as_ref()?.repos.get(index)
    }

    fn settings_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .child("Settings")
            .child("Personalize this workspace. The linked ArtCraft snapshot stays read-only.")
            .child(
                Form::new()
                    .child(
                        Field::new()
                            .label("Installer file extension")
                            .child(Input::new(&self.extension_input)),
                    )
                    .child(
                        Field::new().label_indent(false).child(
                            Checkbox::new("prereleases")
                                .label("Include pre-releases")
                                .checked(self.settings.include_prereleases)
                                .on_change(cx.listener(|this, value, _, cx| {
                                    this.settings.include_prereleases = *value;
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        Field::new().label_indent(false).child(
                            Checkbox::new("archived")
                                .label("Show archived repositories")
                                .checked(self.settings.show_archived)
                                .on_change(cx.listener(|this, value, _, cx| {
                                    this.settings.show_archived = *value;
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .child("Appearance")
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("appearance-system")
                            .label(if self.settings.appearance == Appearance::System {
                                "● System"
                            } else {
                                "System"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_appearance(Appearance::System, window, cx)
                            })),
                    )
                    .child(
                        Button::new("appearance-light")
                            .label(if self.settings.appearance == Appearance::Light {
                                "● Light"
                            } else {
                                "Light"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_appearance(Appearance::Light, window, cx)
                            })),
                    )
                    .child(
                        Button::new("appearance-dark")
                            .label(if self.settings.appearance == Appearance::Dark {
                                "● Dark"
                            } else {
                                "Dark"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_appearance(Appearance::Dark, window, cx)
                            })),
                    ),
            )
            .child(
                Button::new("save-settings")
                    .primary()
                    .label("Save settings")
                    .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
            )
    }

    fn repo_detail(&self) -> impl IntoElement {
        let mut detail = div().flex().flex_col().gap_3().p_6();
        let Some(repo) = self.selected_repo() else {
            return detail.child("Select a repository from the list.");
        };
        detail = detail
            .child(repo.name.clone())
            .child(repo.description.clone().unwrap_or_default())
            .child(format!("GitHub: {}", repo.url))
            .child(format!(
                "Visibility: {}",
                if repo.is_private { "Private" } else { "Public" }
            ))
            .child(format!(
                "Default branch: {}",
                repo.default_branch_ref
                    .as_ref()
                    .map_or("Unknown", |branch| &branch.name)
            ))
            .child(format!(
                "Pushed: {}",
                repo.pushed_at.as_deref().unwrap_or("Unknown")
            ));

        if let Some((release, assets)) = latest_installer(
            repo,
            self.settings.include_prereleases,
            &self.settings.installer_extension,
        ) {
            detail = detail
                .child(format!("Latest installer release: {}", release.tag_name))
                .child(format!(
                    "Published: {}",
                    release
                        .published_at
                        .as_deref()
                        .unwrap_or(&release.created_at)
                ))
                .child(format!("Release: {}", release.url));
            for asset in assets {
                detail = detail.child(format!("{} · {} bytes", asset.name, asset.size));
            }
        } else {
            detail = detail.child(format!(
                "No .{} installer release found.",
                self.settings.installer_extension
            ));
        }
        detail
    }

    fn repositories_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(270.))
            .h_full()
            .overflow_y_scrollbar()
            .p_3();
        if let Some(snapshot) = &self.snapshot {
            for (index, repo) in snapshot.repos.iter().enumerate() {
                if repo.is_archived && !self.settings.show_archived {
                    continue;
                }
                let label = if Some(index) == self.selected {
                    format!("● {}", repo.name)
                } else {
                    repo.name.clone()
                };
                list = list.child(Button::new(format!("repo-{index}")).label(label).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.selected = Some(index);
                        cx.notify();
                    }),
                ));
            }
        }
        div().flex().size_full().child(list).child(
            div()
                .flex_1()
                .h_full()
                .overflow_y_scrollbar()
                .child(self.repo_detail()),
        )
    }

    fn installers_view(&self) -> impl IntoElement {
        let mut shelf = div().flex().flex_col().gap_3().p_6().overflow_y_scrollbar().size_full()
            .child("Installer shelf")
            .child("Eligible releases in the linked snapshot. Downloads will live in the managed library.");
        if let Some(snapshot) = &self.snapshot {
            let mut count = 0;
            for repo in &snapshot.repos {
                if let Some((release, assets)) = latest_installer(
                    repo,
                    self.settings.include_prereleases,
                    &self.settings.installer_extension,
                ) {
                    count += 1;
                    shelf = shelf.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_4()
                            .rounded_lg()
                            .child(format!("{}  ·  {}", repo.name, release.tag_name))
                            .child(
                                assets
                                    .iter()
                                    .map(|asset| {
                                        format!("{} · {} MB", asset.name, asset.size / 1_000_000)
                                    })
                                    .collect::<Vec<_>>()
                                    .join("   "),
                            ),
                    );
                }
            }
            if count == 0 {
                shelf = shelf.child("No installer releases found.");
            }
        }
        shelf
    }
}

impl Render for ReleaseDesk {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.page != Page::Welcome;
        div()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_4()
                    .w(px(190.))
                    .h_full()
                    .bg(cx.theme().secondary)
                    .child("RELEASE DESK")
                    .child(if ready { "ARTCRAFT" } else { "SETUP" })
                    .child(
                        Button::new("overview")
                            .label("Overview")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.snapshot.is_some() {
                                    this.page = Page::Overview;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("repositories")
                            .label("Repositories")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.snapshot.is_some() {
                                    this.page = Page::Repositories;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("installers")
                            .label("Installers")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.snapshot.is_some() {
                                    this.page = Page::Installers;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("settings")
                            .label("Settings")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.snapshot.is_some() {
                                    this.page = Page::Settings;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("refresh")
                            .label("Refresh snapshot")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.snapshot.is_some() {
                                    this.refresh_snapshot(cx);
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .child(div().p_4().bg(cx.theme().muted).child(self.message.clone()))
                    .child(match self.page {
                        Page::Welcome => self.welcome_view(cx).into_any_element(),
                        Page::Overview => self.overview_view(cx).into_any_element(),
                        Page::Repositories => self.repositories_view(cx).into_any_element(),
                        Page::Installers => self.installers_view().into_any_element(),
                        Page::Settings => self.settings_view(cx).into_any_element(),
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
