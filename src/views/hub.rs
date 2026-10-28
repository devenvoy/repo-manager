//! Project hub: the starting point for all independent workspaces.

use crate::components::motion_button::Button;
use crate::components::provider_icon::provider_icon;
use crate::components::ui_icons::folder_icon;
use crate::workspace::WorkspaceProject;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use release_desk_core::project::DiskLayout;

pub fn render_hub<
    T: 'static,
    FNew: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FOpen: Fn(u128, &mut T, &mut Context<T>) + Copy + 'static,
>(
    projects: &[WorkspaceProject],
    cx: &Context<T>,
    on_new: FNew,
    on_open: FOpen,
) -> impl IntoElement {
    let mut cards = div().flex().flex_wrap().gap_4();
    for project in projects {
        cards = cards.child(render_project_card(project, cx, on_open));
    }

    let total_sources: usize = projects.iter().map(|p| p.sources.len()).sum();
    let total_repos: usize = projects
        .iter()
        .map(|p| {
            p.sources
                .iter()
                .map(|s| s.repositories.iter().filter(|r| r.selected).count())
                .sum::<usize>()
        })
        .sum();

    div()
        .flex()
        .flex_col()
        .gap_6()
        .p_6()
        .overflow_y_scrollbar()
        // Page Head
        .child(
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
                                .text_size(px(10.))
                                .font_bold()
                                .text_color(cx.theme().primary)
                                .child("PROJECT FLEET HUB"),
                        )
                        .child(
                            div()
                                .text_size(px(26.))
                                .font_bold()
                                .text_color(cx.theme().foreground)
                                .child("Projects, all in one place."),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Each project binds to a local folder with its own Git provider, account, sources, and repositories."),
                        ),
                )
                .child(
                    Button::new("new-project")
                        .label("+ New project")
                        .primary()
                        .on_click(cx.listener(move |this, _, window, cx| on_new(this, window, cx))),
                ),
        )
        // Hub Content
        .child(if projects.is_empty() {
            div()
                .p_6()
                .rounded_2xl()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .items_center()
                .justify_between()
                .gap_6()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap_3()
                        .max_w(px(580.))
                        .child(
                            div()
                                .text_size(px(10.))
                                .font_bold()
                                .text_color(cx.theme().primary)
                                .child("START HERE"),
                        )
                        .child(
                            div()
                                .text_size(px(22.))
                                .font_bold()
                                .text_color(cx.theme().foreground)
                                .child("Build a workspace around a folder."),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Release Desk manages multiple Git projects on your machine. Each project connects to any Git host (GitHub, GitLab, Bitbucket, Gitea, etc.), ingests creator accounts, and aggregates all release apps into your local store."),
                        )
                        .child(
                            Button::new("hero-create-project")
                                .label("+ Create Your First Project")
                                .primary()
                                .on_click(cx.listener(move |this, _, window, cx| on_new(this, window, cx))),
                        ),
                )
                .child(
                    div()
                        .w(px(72.))
                        .h(px(72.))
                        .rounded_2xl()
                        .bg(cx.theme().primary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(folder_icon()),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_5()
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(stat_pill("Projects", &projects.len().to_string(), cx))
                        .child(stat_pill("Source Profiles", &total_sources.to_string(), cx))
                        .child(stat_pill("Tracked Repos", &total_repos.to_string(), cx)),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .font_bold()
                        .text_color(cx.theme().foreground)
                        .child(format!("All projects  ·  {}", projects.len())),
                )
                .child(cards)
                .into_any_element()
        })
}

fn stat_pill<T: 'static>(label: &str, value: &str, cx: &Context<T>) -> impl IntoElement {
    div()
        .p_3()
        .rounded_lg()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_col()
        .gap_1()
        .min_w(px(120.))
        .child(
            div()
                .text_size(px(18.))
                .font_bold()
                .text_color(cx.theme().foreground)
                .child(value.to_string()),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
}

fn render_project_card<T: 'static, FOpen: Fn(u128, &mut T, &mut Context<T>) + Copy + 'static>(
    project: &WorkspaceProject,
    cx: &Context<T>,
    on_open: FOpen,
) -> impl IntoElement {
    let id = project.id;
    let layout = match project.disk_layout {
        DiskLayout::Grouped => "Owner / repository",
        DiskLayout::Flat => "Flat on disk",
    };
    div()
        .id(format!("project-card-{id}"))
        .cursor_pointer()
        .w(px(330.))
        .p_5()
        .rounded_xl()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .hover(|s| s.border_color(cx.theme().primary))
        .flex()
        .flex_col()
        .gap_4()
        .on_click(cx.listener(move |this, _, _, cx| on_open(id, this, cx)))
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(provider_icon(project.provider))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(17.))
                                .font_bold()
                                .text_color(cx.theme().foreground)
                                .child(project.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} · {}",
                                    project.provider.label(),
                                    project.account
                                )),
                        ),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(project.directory.display().to_string()),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child(layout),
        )
        .child(
            div()
                .text_size(px(12.))
                .font_bold()
                .text_color(cx.theme().primary)
                .child("Open project  →"),
        )
}
