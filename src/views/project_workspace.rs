//! Selected project workspace and its settings boundary.

use crate::components::provider_icon::provider_icon;
use crate::workspace::{ProjectSection, WorkspaceProject};
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, Context, IntoElement, ParentElement as _, Styled as _, Window, div, px,
};
use release_desk_core::project::DiskLayout;

#[allow(clippy::too_many_arguments)] // One renderer boundary carries the selected project and four UI actions.
pub fn render_project_workspace<
    T: 'static,
    FSection: Fn(ProjectSection, &mut T, &mut Context<T>) + Copy + 'static,
    FBack: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FEdit: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FDelete: Fn(&mut T, &mut Context<T>) + Copy + 'static,
>(
    project: &WorkspaceProject,
    section: ProjectSection,
    delete_confirm: bool,
    section_content: Option<AnyElement>,
    cx: &Context<T>,
    on_section: FSection,
    on_back: FBack,
    on_edit: FEdit,
    on_delete: FDelete,
) -> impl IntoElement {
    let mut tabs = div()
        .flex()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border);
    for tab in ProjectSection::ALL {
        tabs = tabs.child(
            Button::new(format!("section-{tab:?}"))
                .label(if tab == section {
                    format!("● {}", tab.label())
                } else {
                    tab.label().into()
                })
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| on_section(tab, this, cx))),
        );
    }

    let body = match section {
        ProjectSection::Overview => div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_size(px(20.)).font_bold().child("Project overview"))
            .child(detail_row("Provider", project.provider.label(), cx))
            .child(detail_row("Account", &project.account, cx))
            .child(detail_row("Folder", &project.directory.display().to_string(), cx))
            .child(detail_row("Disk layout", if project.disk_layout == DiskLayout::Grouped { "Grouped by owner" } else { "Flat" }, cx))
            .into_any_element(),
        ProjectSection::Sources | ProjectSection::Repositories | ProjectSection::Releases => section_content.unwrap_or_else(|| empty_section("No content yet", "Add sources and repositories to see details here.", cx).into_any_element()),
        ProjectSection::Settings => div()
            .flex()
            .flex_col()
            .gap_5()
            .child(div().text_size(px(20.)).font_bold().child("Project settings"))
            .child(detail_row("Folder", &project.directory.display().to_string(), cx))
            .child(detail_row("Provider", project.provider.label(), cx))
            .child(detail_row("Account", &project.account, cx))
            .child(Button::new("edit-project").label("Edit project setup").primary().on_click(cx.listener(move |this, _, window, cx| on_edit(this, window, cx))))
            .child(div().pt_4().border_t_1().border_color(cx.theme().border).text_size(px(16.)).font_bold().child("Delete project"))
            .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child("Remove this project from Release Desk. The directory and its files stay on disk."))
            .child(Button::new("delete-project").label(if delete_confirm { "Confirm: remove project" } else { "Delete project" }).on_click(cx.listener(move |this, _, _, cx| on_delete(this, cx))))
            .into_any_element(),
    };

    div()
        .flex()
        .flex_col()
        .gap_6()
        .p_6()
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
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
                                        .text_size(px(27.))
                                        .font_bold()
                                        .child(project.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{} · {}",
                                            project.provider.label(),
                                            project.directory.display()
                                        )),
                                ),
                        ),
                )
                .child(
                    Button::new("back-to-projects")
                        .label("← All projects")
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| on_back(this, cx))),
                ),
        )
        .child(tabs)
        .child(
            div()
                .p_5()
                .rounded_xl()
                .bg(cx.theme().secondary)
                .child(body),
        )
}

fn detail_row<T: 'static>(label: &str, value: &str, cx: &Context<T>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .py_2p5()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(px(12.))
                .font_bold()
                .child(value.to_string()),
        )
}

fn empty_section<T: 'static>(title: &str, message: &str, cx: &Context<T>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(px(20.))
                .font_bold()
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(message.to_string()),
        )
}

/// Render the Release shelf for a project with multi-platform filters and installer destination note.
pub fn render_project_releases<
    T: 'static,
    FPlatform: Fn(&'static str, &mut T, &mut Context<T>) + Copy + 'static,
    FDownload: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
>(
    project: &WorkspaceProject,
    current_platform: &'static str,
    cx: &Context<T>,
    on_platform: FPlatform,
    on_download: FDownload,
) -> impl IntoElement {
    let platforms = ["macOS", "Windows", "Linux", "All"];
    let mut platform_bar = div().flex().gap_1().p_1().rounded_lg().bg(cx.theme().muted);
    for platform in platforms {
        let is_selected = current_platform == platform;
        platform_bar = platform_bar.child(
            Button::new(format!("platform-{platform}"))
                .label(if is_selected {
                    format!("● {platform}")
                } else {
                    platform.to_string()
                })
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| on_platform(platform, this, cx))),
        );
    }

    let installers_path = project.directory.join(".release-desk").join("installers");

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_size(px(11.)).font_bold().text_color(cx.theme().primary).child("RELEASES"))
                        .child(div().text_size(px(20.)).font_bold().child("Release shelf"))
                        .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(format!("Downloaded assets are saved in: {}", installers_path.display()))),
                )
                .child(platform_bar),
        )
        .child(
            div()
                .p_4()
                .rounded_lg()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().font_bold().child(format!("{} builds for {}", current_platform, project.name)))
                        .child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground).child("Per-project storage")),
                )
                .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child("Select repositories and fetch upstream release tags to manage and stage build installers."))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .pt_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{}-latest-asset", project.name.to_lowercase().replace(' ', "-"))),
                        )
                        .child(
                            Button::new("download-sample-asset")
                                .label("Stage in .release-desk/installers/")
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_download("latest-build".into(), this, cx);
                                })),
                        ),
                ),
        )
}
