//! Selected project workspace and its settings boundary.

use crate::components::card::{property_row, summary_stat_card};
use crate::components::motion_button::Button;
use crate::components::provider_icon::provider_icon;
use crate::styles::CssExt as _;
use crate::workspace::{ProjectSection, WorkspaceProject};
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::{
    AnyElement, Context, IntoElement, ParentElement as _, Styled as _, Window, div, px,
};

#[allow(clippy::too_many_arguments)] // One renderer boundary carries the selected project and UI actions.
pub fn render_project_workspace<
    T: 'static,
    FSection: Fn(ProjectSection, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FBack: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FEdit: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FDelete: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FImport: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
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
    on_import: FImport,
) -> impl IntoElement {
    let mut tabs = div()
        .flex()
        .flex_wrap()
        .gap_1()
        .border_b_1()
        .border_color(cx.theme().border);
    for tab in ProjectSection::ALL {
        let is_active = tab == section;
        tabs = tabs.child(
            div()
                .border_b_2()
                .border_color(if is_active {
                    cx.theme().primary
                } else {
                    cx.theme().background
                })
                .child(
                    Button::new(format!("section-{tab:?}"))
                        .label(tab.label().to_string())
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_section(tab, this, window, cx)
                        })),
                ),
        );
    }

    let body = match section {
        ProjectSection::Overview => {
            let discovered: usize = project.sources.iter().map(|source| source.repositories.len()).sum();
            let selected: usize = project.sources.iter().flat_map(|source| &source.repositories).filter(|repo| repo.selected).count();
            div()
                .flex().flex_col().gap_5()
                .child(div().text_size(px(12.)).font_bold().text_color(cx.theme().primary).child("YOUR WORKSPACE"))
                .child(div().text_size(px(27.)).font_bold().child("Your repositories, together."))
                .child(div().text_size(px(14.)).text_color(cx.theme().muted_foreground).child("Browse the repositories you discover, choose what to manage, and keep your local copies up to date."))
                .child(div().flex().flex_wrap().gap_3()
                    .child(summary_stat_card("Sources", project.sources.len(), cx.theme()))
                    .child(summary_stat_card("Discovered repositories", discovered, cx.theme()))
                    .child(summary_stat_card("Selected to manage", selected, cx.theme())))
                .child(div().flex().flex_wrap().gap_2()
                    .child(Button::new("overview-sources").label(if project.sources.is_empty() { "Add your first source" } else { "Browse sources" }).primary().on_click(cx.listener(move |this, _, window, cx| on_section(ProjectSection::Sources, this, window, cx))))
                    .child(Button::new("overview-repos").label("View repositories").outline().on_click(cx.listener(move |this, _, window, cx| on_section(ProjectSection::Repositories, this, window, cx))))
                    .child(Button::new("overview-releases").label("View releases").ghost().on_click(cx.listener(move |this, _, window, cx| on_section(ProjectSection::Releases, this, window, cx)))))
                .child(div().pt_3().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(format!("Project folder · {}", project.directory.display())))
                .into_any_element()
        },
        ProjectSection::Sources | ProjectSection::Repositories | ProjectSection::Releases => section_content.unwrap_or_else(|| empty_section("No content yet", "Add sources and repositories to see details here.", cx).into_any_element()),
        ProjectSection::Settings => div()
            .flex()
            .flex_col()
            .gap_5()
            .child(div().text_size(px(20.)).font_bold().child("Project settings"))
            .child(property_row("Folder", &project.directory.display().to_string(), cx.theme()))
            .child(property_row("Provider", project.provider.label(), cx.theme()))
            .child(property_row("Account", &project.account, cx.theme()))
            .child(Button::new("edit-project").label("Edit project setup").primary().on_click(cx.listener(move |this, _, window, cx| on_edit(this, window, cx))))
            .child(
                div()
                    .pt_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_size(px(16.)).font_bold().child("Import Existing Repositories"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Scan your project folder to discover and import existing local Git repositories (such as ArtCraft or local working projects)."),
                    )
                    .child(
                        Button::new("settings-import-local-repos")
                            .icon(crate::components::ui_icons::repository_icon())
                            .label("Scan project folder for repos")
                            .outline()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_import(this, window, cx)
                            })),
                    ),
            )
            .child(div().pt_4().border_t_1().border_color(cx.theme().border).text_size(px(16.)).font_bold().child("Delete project"))
            .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child("Remove this project from Release Desk. The directory and its files stay on disk."))
            .child(Button::new("delete-project").label(if delete_confirm { "Confirm: remove project" } else { "Delete project" }).on_click(cx.listener(move |this, _, _, cx| on_delete(this, cx))))
            .into_any_element(),
    };

    div()
        .css_page_scroll()
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
            if matches!(section, ProjectSection::Overview | ProjectSection::Settings) {
                div()
                    .p_6()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(body)
                    .into_any_element()
            } else {
                div().flex_1().min_h_0().child(body).into_any_element()
            },
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
