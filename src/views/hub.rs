//! Project Hub view displaying multi-project workspaces, isolated provider authentication,
//! and profile ingestion with repository checkbox picker.

use crate::settings::HubViewMode;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};
use release_desk_core::project::{ProjectIdentity, SelectableRepo};

/// Encapsulated input properties for the Project Hub view.
pub struct HubViewProps<'a> {
    pub projects: &'a [ProjectIdentity],
    pub view_mode: HubViewMode,
    pub selectable_repos: &'a [SelectableRepo],
    pub show_ingest_modal: bool,
}

/// Encapsulated event handlers for user actions within the Project Hub.
#[derive(Clone, Copy)]
pub struct HubViewHandlers<FMode, FNew, FIngest, FSync, FToggle, FSelectAll, FConfirm, FClose> {
    pub on_toggle_view_mode: FMode,
    pub on_new_project: FNew,
    pub on_open_ingest: FIngest,
    pub on_sync_project: FSync,
    pub on_toggle_repo: FToggle,
    pub on_select_all_repos: FSelectAll,
    pub on_confirm_import: FConfirm,
    pub on_close_ingest: FClose,
}

pub fn render_hub<
    T: 'static,
    FMode: Fn(HubViewMode, &mut T, &mut Context<T>) + Copy + 'static,
    FNew: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FIngest: Fn(u128, &mut T, &mut Context<T>) + Copy + 'static,
    FSync: Fn(u128, &mut T, &mut Context<T>) + Copy + 'static,
    FToggle: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FSelectAll: Fn(bool, &mut T, &mut Context<T>) + Copy + 'static,
    FConfirm: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FClose: Fn(&mut T, &mut Context<T>) + Copy + 'static,
>(
    props: HubViewProps<'_>,
    handlers: HubViewHandlers<FMode, FNew, FIngest, FSync, FToggle, FSelectAll, FConfirm, FClose>,
    cx: &Context<T>,
) -> impl IntoElement {
    let on_toggle_view_mode = handlers.on_toggle_view_mode;
    let on_new_project = handlers.on_new_project;
    let on_open_ingest = handlers.on_open_ingest;
    let on_sync_project = handlers.on_sync_project;
    let on_toggle_repo = handlers.on_toggle_repo;
    let on_select_all_repos = handlers.on_select_all_repos;
    let on_confirm_import = handlers.on_confirm_import;
    let on_close_ingest = handlers.on_close_ingest;
    let projects = props.projects;
    let view_mode = props.view_mode;
    let selectable_repos = props.selectable_repos;
    let show_ingest_modal = props.show_ingest_modal;
    let mut root = div().flex().flex_col().gap_6().p_6().overflow_y_scrollbar();

    // 1. TOP HEADER & VIEW MODE CONTROLLER
    root = root.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_bold()
                            .child("Project Workspaces"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Manage projects bound to disk folders with isolated Git provider credentials."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .p_1()
                            .rounded_lg()
                            .bg(cx.theme().muted)
                            .child(
                                Button::new("mode-grouped")
                                    .label("Grouped")
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        on_toggle_view_mode(HubViewMode::Grouped, this, cx);
                                    })),
                            )
                            .child(
                                Button::new("mode-flat")
                                    .label("Flat")
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        on_toggle_view_mode(HubViewMode::Flat, this, cx);
                                    })),
                            ),
                    )
                    .child(
                        Button::new("new-project-btn")
                            .label("+ New Project")
                            .primary()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_new_project(this, cx);
                            })),
                    ),
            ),
    );

    // 2. PROJECT CARDS GRID
    let mut grid = div().flex().flex_wrap().gap_4();
    if projects.is_empty() {
        grid = grid.child(
            div()
                .p_6()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .text_color(cx.theme().muted_foreground)
                .child(
                    "No projects added yet. Click '+ New Project' to bind a workspace directory.",
                ),
        );
    } else {
        for project in projects {
            grid = grid.child(render_project_card(
                project,
                view_mode,
                cx,
                on_open_ingest,
                on_sync_project,
            ));
        }
    }
    root = root.child(grid);

    // 3. INGEST USER / ORG REPO CHECKLIST MODAL
    if show_ingest_modal {
        root = root.child(render_ingest_modal(
            selectable_repos,
            cx,
            on_toggle_repo,
            on_select_all_repos,
            on_confirm_import,
            on_close_ingest,
        ));
    }

    root
}

fn render_project_card<T: 'static>(
    project: &ProjectIdentity,
    view_mode: HubViewMode,
    cx: &Context<T>,
    on_open_ingest: impl Fn(u128, &mut T, &mut Context<T>) + Copy + 'static,
    on_sync_project: impl Fn(u128, &mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    let proj_id = project.id.as_u128();
    let location_str = project
        .clone_location
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "Not bound".to_string());

    let layout_badge = match view_mode {
        HubViewMode::Grouped => "Grouped Layout (<user>/<repo>)",
        HubViewMode::Flat => "Flat Layout (<repo>)",
    };

    div()
        .w(px(360.))
        .p_5()
        .rounded_xl()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_bold()
                                .child(project.name.as_str().to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("📁 {location_str}")),
                        ),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .bg(cx.theme().secondary)
                        .text_color(cx.theme().secondary_foreground)
                        .text_size(px(10.))
                        .font_bold()
                        .child("GitHub"),
                ),
        )
        .child(
            div()
                .p_2p5()
                .rounded_lg()
                .bg(cx.theme().muted)
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(format!("Active Mode: {layout_badge}")),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .pt_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new(format!("btn-ingest-{}", proj_id))
                        .label("+ Ingest Profile")
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_open_ingest(proj_id, this, cx);
                        })),
                )
                .child(
                    Button::new(format!("btn-sync-{}", proj_id))
                        .label("Sync Fleet")
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_sync_project(proj_id, this, cx);
                        })),
                ),
        )
}

fn render_ingest_modal<T: 'static>(
    repos: &[SelectableRepo],
    cx: &Context<T>,
    on_toggle_repo: impl Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    on_select_all: impl Fn(bool, &mut T, &mut Context<T>) + Copy + 'static,
    on_confirm_import: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
    on_close_ingest: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    div()
        .p_6()
        .rounded_2xl()
        .bg(cx.theme().secondary)
        .border_2()
        .border_color(cx.theme().primary)
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(18.))
                                .font_bold()
                                .child("Select Repositories to Clone & Manage"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Choose public and permitted private repos discovered from profile."),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("select-all-btn")
                                .label("Select All")
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_select_all(true, this, cx);
                                })),
                        )
                        .child(
                            Button::new("deselect-all-btn")
                                .label("Deselect All")
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_select_all(false, this, cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .max_h(px(240.))
                .overflow_y_scrollbar()
                .children(repos.iter().map(|repo| {
                    let rname = repo.name.clone();
                    let check_mark = if repo.selected { "☑" } else { "☐" };
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .p_2p5()
                        .rounded_lg()
                        .bg(cx.theme().muted)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_bold()
                                        .child(check_mark),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_bold()
                                                .child(repo.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(10.))
                                                .text_color(cx.theme().muted_foreground)
                                                .child(repo.description.clone().unwrap_or_default()),
                                        ),
                                ),
                        )
                        .child(
                            Button::new(format!("toggle-{}", repo.name))
                                .label("Toggle")
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_toggle_repo(rname.clone(), this, cx);
                                })),
                        )
                })),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .pt_3()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new("cancel-ingest-btn")
                        .label("Cancel")
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_close_ingest(this, cx);
                        })),
                )
                .child(
                    Button::new("confirm-import-btn")
                        .label("Import & Clone Selected")
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_confirm_import(this, cx);
                        })),
                ),
        )
}
