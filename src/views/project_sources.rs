//! Project source selection, repository list views, and clone review modal.

use crate::workspace_sources::{RepositoryListMode, WorkspaceSource};
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{Context, Entity, IntoElement, ParentElement as _, Styled as _, div, px};
use release_desk_application::use_case::profile_ingestion::RepoClonePlan;
use release_desk_core::project::DiskLayout;
use std::path::Path;

/// Render source entry and repository selection.
#[allow(clippy::too_many_arguments)]
pub fn render_project_sources<
    T: 'static,
    FAdd: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FRemove: Fn(usize, &mut T, &mut Context<T>) + Copy + 'static,
    FToggle: Fn(usize, usize, &mut T, &mut Context<T>) + Copy + 'static,
    FSelectAll: Fn(usize, bool, &mut T, &mut Context<T>) + Copy + 'static,
    FReview: Fn(&mut T, &mut Context<T>) + Copy + 'static,
>(
    sources: &[WorkspaceSource],
    input: &Entity<InputState>,
    cx: &Context<T>,
    on_add: FAdd,
    on_remove: FRemove,
    on_toggle: FToggle,
    on_select_all: FSelectAll,
    on_review: FReview,
) -> impl IntoElement {
    let mut list = div().flex().flex_col().gap_4();
    let mut total_selected = 0;

    for (source_index, source) in sources.iter().enumerate() {
        let mut repos = div().flex().flex_col().gap_1();
        let all_selected =
            !source.repositories.is_empty() && source.repositories.iter().all(|r| r.selected);

        for (repo_index, repo) in source.repositories.iter().enumerate() {
            if repo.selected {
                total_selected += 1;
            }
            repos = repos.child(
                Button::new(format!("source-{source_index}-repo-{repo_index}"))
                    .label(format!(
                        "{} {}",
                        if repo.selected { "☑" } else { "☐" },
                        repo.name
                    ))
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        on_toggle(source_index, repo_index, this, cx)
                    })),
            );
        }
        list = list.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(div().font_bold().child(source.handle.clone()))
                                .child(
                                    Button::new(format!("select-all-source-{source_index}"))
                                        .label(if all_selected {
                                            "Deselect All"
                                        } else {
                                            "Select All"
                                        })
                                        .ghost()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            on_select_all(source_index, !all_selected, this, cx)
                                        })),
                                ),
                        )
                        .child(
                            Button::new(format!("remove-source-{source_index}"))
                                .label("Remove")
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_remove(source_index, this, cx)
                                })),
                        ),
                )
                .child(repos),
        );
    }

    let mut container = div().flex().flex_col().gap_4()
        .child(div().text_size(px(20.)).font_bold().child("People and organizations"))
        .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child("Add a creator or team by username or profile URL to review and select their repositories."))
        .child(div().flex().gap_2().child(div().flex_1().child(Input::new(input)))
            .child(Button::new("add-source").label("Add source").primary().on_click(cx.listener(move |this, _, _, cx| on_add(this, cx)))))
        .child(list);

    if total_selected > 0 {
        container = container.child(
            div()
                .p_4()
                .rounded_xl()
                .bg(cx.theme().muted)
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(px(13.)).font_bold().child(format!(
                    "{total_selected} repositories selected for this project"
                )))
                .child(
                    Button::new("review-clones-btn")
                        .label("Review Clone Plan →")
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| on_review(this, cx))),
                ),
        );
    }

    container
}

/// Render the Clone Review Modal showing exact planned disk paths before execution.
pub fn render_clone_review_modal<
    T: 'static,
    FConfirm: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FCancel: Fn(&mut T, &mut Context<T>) + Copy + 'static,
>(
    project_name: &str,
    project_dir: &Path,
    disk_layout: DiskLayout,
    plans: &[RepoClonePlan],
    cx: &Context<T>,
    on_confirm: FConfirm,
    on_cancel: FCancel,
) -> impl IntoElement {
    let layout_text = match disk_layout {
        DiskLayout::Grouped => "Grouped (<project>/<owner>/<repo>)",
        DiskLayout::Flat => "Flat (<project>/<repo>)",
    };

    let mut plan_list = div().flex().flex_col().gap_2();
    for plan in plans {
        plan_list = plan_list.child(
            div()
                .p_3()
                .rounded_lg()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_bold()
                                .text_size(px(13.))
                                .child(plan.repo_name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(cx.theme().primary)
                                .child("READY"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Destination: {}", plan.target_path.display())),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Remote: {}", plan.clone_url)),
                ),
        );
    }

    div()
        .p_6()
        .rounded_xl()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_bold()
                        .text_color(cx.theme().primary)
                        .child("PRE-FLIGHT CONFIRMATION"),
                )
                .child(
                    div()
                        .text_size(px(20.))
                        .font_bold()
                        .child(format!("Confirm Clones for {project_name}")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "Target folder: {} · Layout: {}",
                            project_dir.display(),
                            layout_text
                        )),
                ),
        )
        .child(plan_list)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt_3()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new("cancel-clone-plan")
                        .label("Cancel")
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| on_cancel(this, cx))),
                )
                .child(
                    Button::new("confirm-clone-plan")
                        .label(format!("Confirm & Clone {} Repositories", plans.len()))
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| on_confirm(this, cx))),
                ),
        )
}

/// Render selected repositories using a presentation preference separate from disk layout.
pub fn render_project_repositories<
    T: 'static,
    FMode: Fn(RepositoryListMode, &mut T, &mut Context<T>) + Copy + 'static,
>(
    sources: &[WorkspaceSource],
    mode: RepositoryListMode,
    cx: &Context<T>,
    on_mode: FMode,
) -> impl IntoElement {
    let mut list = div().flex().flex_col().gap_3();
    let mut count = 0;
    for source in sources {
        let selected: Vec<_> = source
            .repositories
            .iter()
            .filter(|repo| repo.selected)
            .collect();
        if selected.is_empty() {
            continue;
        }
        if mode == RepositoryListMode::Grouped {
            list = list.child(div().font_bold().child(source.handle.clone()));
        }
        for repo in selected {
            count += 1;
            let name = if mode == RepositoryListMode::Flat {
                format!("{}/{}", source.handle, repo.name)
            } else {
                repo.name.clone()
            };
            list = list.child(
                div()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(name),
            );
        }
    }
    if count == 0 {
        list = list.child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child("Select repositories in Sources to see them here."),
        );
    }
    div().flex().flex_col().gap_4()
        .child(div().flex().items_center().justify_between()
            .child(div().text_size(px(20.)).font_bold().child("Selected repositories"))
            .child(div().flex().gap_2()
                .child(Button::new("repos-grouped").label(if mode == RepositoryListMode::Grouped { "● Grouped" } else { "Grouped" }).on_click(cx.listener(move |this, _, _, cx| on_mode(RepositoryListMode::Grouped, this, cx))))
                .child(Button::new("repos-flat").label(if mode == RepositoryListMode::Flat { "● Flat" } else { "Flat" }).on_click(cx.listener(move |this, _, _, cx| on_mode(RepositoryListMode::Flat, this, cx))))))
        .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child("This display setting is independent of the folder layout on disk. Repository names are examples until discovery is connected."))
        .child(list)
}
