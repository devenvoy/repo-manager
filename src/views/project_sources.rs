//! Source profiles and repository selection dialogs.

use crate::components::icons::{branch_icon, fork_icon, plus_icon, star_icon, trash_icon};
use crate::components::motion_button::Button;
use crate::components::toggle_button;
use crate::workspace_sources::{RepositoryListMode, SourceRepository, WorkspaceSource};
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Animation, AnimationExt as _, Context, Entity, IntoElement, ParentElement as _, Styled as _,
    Window, div, img, px,
};
use release_desk_application::use_case::profile_ingestion::RepoClonePlan;
use release_desk_core::project::DiskLayout;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceDialog {
    #[default]
    Closed,
    Add,
    Select(usize),
    Review(usize),
    ConfirmDelete(usize),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceFilter {
    #[default]
    All,
    Selected,
    Unselected,
}

fn matches_repository(repo: &SourceRepository, query: &str, filter: SourceFilter) -> bool {
    if matches!(filter, SourceFilter::Selected) && !repo.selected
        || matches!(filter, SourceFilter::Unselected) && repo.selected
    {
        return false;
    }
    let query = query.trim().to_lowercase();
    query.is_empty()
        || [&repo.name, &repo.full_name]
            .into_iter()
            .map(String::as_str)
            .chain(repo.description.as_deref())
            .chain(repo.primary_language.as_deref())
            .any(|value| value.to_lowercase().contains(&query))
}

fn provider_choice(provider: &str) -> crate::workspace::ProviderChoice {
    crate::workspace::ProviderChoice::from_source_provider(provider)
}

pub(crate) fn profile_image<T: 'static>(
    source: &WorkspaceSource,
    cx: &Context<T>,
) -> gpui_kit::AnyElement {
    if let Some((url, path)) = source.avatar_cache_request() {
        if path.is_file() {
            return div()
                .relative()
                .w(px(52.))
                .h(px(52.))
                .child(
                    img(path)
                        .w(px(52.))
                        .h(px(52.))
                        .rounded_full()
                        .border_2()
                        .border_color(cx.theme().border),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(-2.))
                        .right(px(-2.))
                        .w(px(20.))
                        .h(px(20.))
                        .rounded_full()
                        .bg(cx.theme().background)
                        .border_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            crate::components::provider_icon::provider_icon(provider_choice(
                                &source.provider,
                            ))
                            .w(px(12.))
                            .h(px(12.)),
                        ),
                )
                .into_any_element();
        } else {
            let path_clone = path.clone();
            std::thread::spawn(move || {
                let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path_clone);
            });
        }
    }

    let initial = source
        .display_name
        .as_deref()
        .or(Some(&source.handle))
        .and_then(|name| name.chars().find(|c| c.is_alphanumeric()))
        .unwrap_or('?')
        .to_uppercase()
        .to_string();

    let choice = provider_choice(&source.provider);
    div()
        .relative()
        .w(px(52.))
        .h(px(52.))
        .child(
            div()
                .w(px(52.))
                .h(px(52.))
                .rounded_full()
                .bg(cx.theme().primary.opacity(0.85))
                .border_2()
                .border_color(cx.theme().border)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(20.))
                .font_bold()
                .text_color(gpui_kit::white())
                .child(initial),
        )
        .child(
            div()
                .absolute()
                .bottom(px(-2.))
                .right(px(-2.))
                .w(px(20.))
                .h(px(20.))
                .rounded_full()
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    crate::components::provider_icon::provider_icon(choice)
                        .w(px(12.))
                        .h(px(12.)),
                ),
        )
        .into_any_element()
}

/// The Sources tab shows people; repository choices live in a separate dialog.
#[allow(clippy::too_many_arguments)]
pub fn render_project_sources<
    T: 'static,
    FAdd: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FRemove: Fn(usize, &mut T, &mut Context<T>) + Copy + 'static,
    FRefresh: Fn(usize, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FToggle: Fn(usize, usize, &mut T, &mut Context<T>) + Copy + 'static,
    FReview: Fn(usize, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FDialog: Fn(SourceDialog, &mut T, &mut Context<T>) + Copy + 'static,
    FOpen: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
>(
    sources: &[WorkspaceSource],
    project_directory: &Path,
    disk_layout: DiskLayout,
    input: &Entity<InputState>,
    search: &Entity<InputState>,
    dialog: SourceDialog,
    busy: bool,
    cx: &Context<T>,
    on_add: FAdd,
    on_remove: FRemove,
    _on_refresh: FRefresh,
    on_toggle: FToggle,
    on_review: FReview,
    on_dialog: FDialog,
    on_open: FOpen,
) -> impl IntoElement {
    let mut people = div().flex().flex_wrap().gap_4().items_start();
    for (source_index, source) in sources.iter().enumerate() {
        let selected = source.repositories.iter().filter(|r| r.selected).count();
        let profile_url = match source.provider.as_str() {
            "github" => format!("https://github.com/{}", source.handle),
            "gitlab" => format!("https://gitlab.com/{}", source.handle),
            "bitbucket" => format!("https://bitbucket.org/{}", source.handle),
            _ => String::new(),
        };
        let mut actions = div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .gap_2();
        let mut left_actions = div().flex().items_center().gap_2();
        if !profile_url.is_empty() {
            left_actions = left_actions.child(
                Button::new(format!("source-provider-{source_index}"))
                    .icon(crate::components::provider_icon::provider_icon(
                        provider_choice(&source.provider),
                    ))
                    .outline()
                    .tooltip(format!("Open @{} on {}", source.handle, source.provider))
                    .on_click(
                        cx.listener(move |this, _, _, cx| on_open(profile_url.clone(), this, cx)),
                    ),
            );
        }
        left_actions = left_actions.child(
            Button::new(format!("source-repos-{source_index}"))
                .label(format!("Choose repositories ({selected})"))
                .primary()
                .on_click(cx.listener(move |this, _, _, cx| {
                    on_dialog(SourceDialog::Select(source_index), this, cx)
                })),
        );
        actions = actions.child(left_actions).child(
            Button::new(format!("source-remove-{source_index}"))
                .icon(trash_icon())
                .tooltip("Remove source")
                .outline()
                .on_click(cx.listener(move |this, _, _, cx| {
                    on_dialog(SourceDialog::ConfirmDelete(source_index), this, cx)
                })),
        );
        let identity = div()
            .flex()
            .items_center()
            .gap_4()
            .child(profile_image(source, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div().font_bold().text_size(px(17.)).child(
                            source
                                .display_name
                                .clone()
                                .unwrap_or_else(|| source.handle.clone()),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("@{} · {}", source.handle, source.provider)),
                    ),
            );
        let bio = source
            .bio
            .as_deref()
            .filter(|bio| !bio.trim().is_empty())
            .unwrap_or("Connected Git profile.");
        people = people.child(
            div()
                .w(px(350.))
                .p_5()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.45))
                .flex()
                .flex_col()
                .gap_4()
                .child(identity)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .line_clamp(2)
                        .child(bio.to_string()),
                )
                .child(
                    div()
                        .pt_3()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} accessible repositories · {selected} selected",
                                    source.repositories.len()
                                )),
                        )
                        .child(actions),
                ),
        );
    }
    if sources.is_empty() {
        people = people.child(
            div()
                .p_6()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .gap_3()
                .child(div().font_bold().text_size(px(16.)).child("No sources yet"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Add a person or organization, then choose the repositories you want to track."),
                )
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .items_center()
                        .pt_2()
                        .child(
                            Button::new("empty-sources-add")
                                .icon(plus_icon())
                                .label("Add source")
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_dialog(SourceDialog::Add, this, cx)
                                })),
                        ),
                ),
        );
    }
    let content = div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(24.))
                                .font_bold()
                                .child("People and organizations"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Profiles connected to this project"),
                        ),
                )
                .child(
                    Button::new("show-add-source")
                        .icon(plus_icon())
                        .label("Add source")
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_dialog(SourceDialog::Add, this, cx)
                        })),
                ),
        )
        .child(people);

    let modal = match dialog {
        SourceDialog::Closed => div().into_any_element(),
        SourceDialog::ConfirmDelete(source_index) => {
            let Some(source) = sources.get(source_index) else { return div().relative().child(content).into_any_element(); };
            div().absolute().inset_0().bg(cx.theme().background.opacity(0.94))
                .flex().items_center().justify_center().p_5()
                .child(div().w(px(430.)).p_6().rounded_xl().border_1().border_color(cx.theme().danger.opacity(0.5))
                    .bg(cx.theme().secondary).flex().flex_col().gap_4()
                    .child(div().font_bold().text_size(px(20.)).child("Remove this source?"))
                    .child(div().text_size(px(13.)).text_color(cx.theme().muted_foreground)
                        .child(format!("@{} and its repository selections will be removed from this project. Local files stay on disk.", source.handle)))
                    .child(div().flex().justify_end().gap_2()
                        .child(Button::new("cancel-delete-source").label("Cancel").outline()
                            .on_click(cx.listener(move |this, _, _, cx| on_dialog(SourceDialog::Closed, this, cx))))
                        .child(Button::new("confirm-delete-source").icon(trash_icon()).label("Remove source").danger()
                            .on_click(cx.listener(move |this, _, _, cx| on_remove(source_index, this, cx)))))
                ).into_any_element()
        }
        SourceDialog::Add => div().absolute().inset_0().bg(cx.theme().background.opacity(0.92))
            .flex().items_center().justify_center().p_5()
            .child(div().w(px(540.)).p_6().rounded_xl().border_1().border_color(cx.theme().border)
                .bg(cx.theme().secondary).flex().flex_col().gap_5()
                .child(div().flex().items_center().justify_between()
                    .child(div().font_bold().text_size(px(21.)).child("Add a source"))
                    .child(Button::new("close-add-source").label("Close").ghost()
                        .on_click(cx.listener(move |this, _, _, cx| on_dialog(SourceDialog::Closed, this, cx)))))
                .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground)
                    .child("Enter a user or organization handle, or paste a provider profile URL."))
                .child(Input::new(input))
                .child(Button::new("confirm-add-source").label(if busy { "Finding repositories…" } else { "Find repositories" })
                    .primary().disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| on_add(this, window, cx)))))
            .into_any_element(),
        SourceDialog::Select(source_index) => {
            if let Some(source) = sources.get(source_index) {
                let query = search.read(cx).value().to_string();
                let mut repos = div().flex().flex_col().gap_2();
                let mut visible = 0;
                let mut already_managed_count = 0;
                let mut actionable_count = 0;
                for (repo_index, repo) in source.repositories.iter().enumerate() {
                    let selected = repo.selected;
                    let clone_path = crate::repository_git::resolve_target_path(
                        project_directory,
                        disk_layout,
                        crate::repository_git::repository_owner(source, repo),
                        &repo.name,
                    );
                    let already_managed = selected && std::fs::symlink_metadata(&clone_path).is_ok();
                    if selected && !already_managed { actionable_count += 1; }
                    if already_managed {
                        already_managed_count += 1;
                        continue;
                    }
                    if !matches_repository(repo, &query, SourceFilter::All) { continue; }
                    visible += 1;
                    let mut header = div().flex().items_center().gap_2().min_w_0();
                    if let Some(browser_url) = source.repository_browser_url(repo) {
                        header = header.child(Button::new(format!("repo-provider-{source_index}-{repo_index}"))
                            .icon(crate::components::provider_icon::provider_icon(provider_choice(&source.provider)))
                            .outline().on_click(cx.listener(move |this, _, _, cx| on_open(browser_url.clone(), this, cx))));
                    }
                    header = header.child(div().flex().flex_col().gap_1().min_w_0()
                        .child(div().font_bold().text_size(px(15.)).child(repo.name.clone()))
                        .child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground).child(repo.full_name.clone())));
                    let mut meta = div().flex().items_center().flex_wrap().gap_4().text_size(px(11.)).text_color(cx.theme().muted_foreground)
                        .child(div().flex().items_center().gap_1().child(star_icon()).child(repo.stars.to_string()))
                        .child(div().flex().items_center().gap_1().child(fork_icon()).child(repo.forks.to_string()))
                        .child(div().flex().items_center().gap_1().child(branch_icon()).child(repo.default_branch.clone()));
                    if let Some(language) = &repo.primary_language { meta = meta.child(language.clone()); }
                    let mut card = div()
                        .p_4()
                        .rounded_lg()
                        .border_2()
                        .border_color(if selected {
                            cx.theme().primary
                        } else {
                            cx.theme().border.opacity(0.6)
                        })
                        .bg(if selected {
                            cx.theme().primary.opacity(0.12)
                        } else {
                            cx.theme().secondary.opacity(0.35)
                        });
                    if selected {
                        card = card.border_l_4().border_color(cx.theme().primary);
                    }
                    repos = repos.child(
                        card.flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_3()
                                    .child(header)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .child(if selected {
                                                div()
                                                    .px_2()
                                                    .py_0p5()
                                                    .rounded_full()
                                                    .bg(cx.theme().primary.opacity(0.2))
                                                    .text_color(cx.theme().primary)
                                                    .text_size(px(11.))
                                                    .font_bold()
                                                    .child("✓ Selected")
                                                    .into_any_element()
                                            } else {
                                                div()
                                                    .px_2()
                                                    .py_0p5()
                                                    .rounded_full()
                                                    .bg(cx.theme().secondary.opacity(0.4))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .text_size(px(11.))
                                                    .child("Unselected")
                                                    .into_any_element()
                                            })
                                            .child(
                                                gpui_kit::component::checkbox::Checkbox::new(
                                                    format!("pick-repo-{source_index}-{repo_index}"),
                                                )
                                                .checked(selected)
                                                .accessibility_label(format!("Select {}", repo.full_name))
                                                .on_change(cx.listener(move |this, _, _, cx| {
                                                    on_toggle(source_index, repo_index, this, cx)
                                                })),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(if selected {
                                        cx.theme().foreground
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .child(
                                        repo.description
                                            .clone()
                                            .unwrap_or_else(|| "No description provided.".into()),
                                    ),
                            )
                            .child(meta),
                    );
                }
                if visible == 0 {
                    repos = repos.child(div().p_5().text_size(px(12.)).text_color(cx.theme().muted_foreground)
                        .child(if already_managed_count == source.repositories.len() {
                            "All repositories from this source are already cloned and selected."
                        } else {
                            "No repositories match your search."
                        }));
                }
                let total_selected = source.repositories.iter().filter(|r| r.selected).count();
                let footer = if total_selected > 0 {
                    div()
                        .pt_4()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().secondary.opacity(0.3))
                        .p_4()
                        .rounded_lg()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(crate::components::icons::folder_icon())
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .font_bold()
                                                .text_size(px(13.))
                                                .child(if actionable_count > 0 {
                                                    format!("{total_selected} selected · {actionable_count} ready to clone")
                                                } else {
                                                    format!("{total_selected} selected · all cloned")
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!(
                                                    "Clone destination: {}",
                                                    project_directory.display()
                                                )),
                                        ),
                                ),
                        )
                        .child(
                            Button::new("review-selected-repositories")
                                .label("Review clone destinations →")
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_dialog(SourceDialog::Review(source_index), this, cx)
                                })),
                        )
                        .with_animation(
                            format!("reveal-clone-review-{source_index}-{total_selected}"),
                            Animation::new(Duration::from_millis(260)),
                            |this, progress| {
                                this.opacity(progress).top(px((1.0 - progress) * 8.0))
                            },
                        )
                        .into_any_element()
                } else {
                    div().into_any_element()
                };
                div().absolute().inset_0().bg(cx.theme().background.opacity(0.96))
                    .flex().items_center().justify_center().p_5()
                    .child(div().w(px(760.)).h(px(590.)).p_5().rounded_xl().border_1().border_color(cx.theme().border)
                        .bg(cx.theme().secondary).flex().flex_col().gap_4()
                        .child(div().flex().items_center().justify_between().gap_3()
                            .child(div().flex().items_center().gap_3()
                                .child(profile_image(source, cx))
                                .child(div().flex().flex_col().gap_1()
                                    .child(div().font_bold().text_size(px(20.)).child(format!("Choose repositories from @{}", source.handle)))
                                    .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground)
                                        .child(format!("{} found · {} already cloned and selected", source.repositories.len(), already_managed_count)))))
                            .child(Button::new("close-repository-picker").label("Done").primary()
                                .on_click(cx.listener(move |this, _, _, cx| on_dialog(SourceDialog::Closed, this, cx)))))
                        .child(Input::new(search))
                        .child(div().flex_1().min_h_0().overflow_y_scrollbar().child(repos))
                        .child(footer))
                    .into_any_element()
            } else { div().into_any_element() }
        }
        SourceDialog::Review(source_index) => {
            let Some(source) = sources.get(source_index) else { return div().relative().child(content).into_any_element(); };
            let mut plans = div().flex().flex_col().gap_2();
            let mut count = 0;
            for repo in &source.repositories {
                if !repo.selected { continue; }
                let target = crate::repository_git::resolve_target_path(
                    project_directory,
                    disk_layout,
                    crate::repository_git::repository_owner(source, repo),
                    &repo.name,
                );
                if std::fs::symlink_metadata(&target).is_ok() { continue; }
                count += 1;
                plans = plans.child(div().p_3().rounded_lg().border_1().border_color(cx.theme().border)
                    .flex().flex_col().gap_1()
                    .child(div().font_bold().child(repo.full_name.clone()))
                    .child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground)
                        .child(target.display().to_string())));
            }
            div().absolute().inset_0().bg(cx.theme().background.opacity(0.96))
                .flex().items_center().justify_center().p_5()
                .child(div().w(px(700.)).h(px(540.)).p_6().rounded_xl().border_1().border_color(cx.theme().border)
                    .bg(cx.theme().secondary).flex().flex_col().gap_4()
                    .child(div().font_bold().text_size(px(21.)).child("Review clone destinations"))
                    .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground)
                        .child(format!("{count} repositories from @{} will be cloned. Existing folders will never be overwritten.", source.handle)))
                    .child(div().flex_1().min_h_0().overflow_y_scrollbar().child(plans))
                    .child(div().pt_4().border_t_1().border_color(cx.theme().border).flex().justify_end().gap_2()
                        .child(Button::new("back-to-repositories").label("Back").outline()
                            .on_click(cx.listener(move |this, _, _, cx| on_dialog(SourceDialog::Select(source_index), this, cx))))
                        .child(Button::new("clone-from-source-dialog").label("Clone selected").primary().disabled(count == 0)
                            .on_click(cx.listener(move |this, _, window, cx| on_review(source_index, this, window, cx)))))
                ).into_any_element()
        }
    };
    div()
        .relative()
        .min_h(px(580.))
        .child(content)
        .child(modal)
        .into_any_element()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DestinationStatus {
    Available,
    Exists,
    Unknown,
}

fn destination_status(path: &Path) -> DestinationStatus {
    match std::fs::symlink_metadata(path) {
        Ok(_) => DestinationStatus::Exists,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DestinationStatus::Available,
        Err(_) => DestinationStatus::Unknown,
    }
}

/// Render the Clone Review Modal showing exact planned disk paths before execution.
pub fn render_clone_review_modal<
    T: 'static,
    FConfirm: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
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
        DiskLayout::Grouped => "Grouped (owner / repository)",
        DiskLayout::Flat => "Flat (repository)",
    };

    let mut plan_list = div().flex().flex_col().gap_2();
    let mut existing_count = 0;
    for plan in plans {
        let status = destination_status(&plan.target_path);
        if status == DestinationStatus::Exists {
            existing_count += 1;
        }
        let status_text = match status {
            DestinationStatus::Available => "No existing item at destination",
            DestinationStatus::Exists => "Destination exists · it will not be overwritten",
            DestinationStatus::Unknown => "Cannot check destination · clone may fail",
        };
        plan_list = plan_list.child(
            div()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .font_bold()
                                .text_size(px(13.))
                                .child(plan.repo_name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(status_text),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Destination: {}", plan.target_path.display())),
                ),
        );
    }
    if plans.is_empty() {
        plan_list = plan_list.child(
            div()
                .text_size(px(13.))
                .text_color(cx.theme().muted_foreground)
                .child("No repositories are selected. Go back to choose repositories."),
        );
    } else if existing_count > 0 {
        plan_list = plan_list.child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(format!("{existing_count} destination(s) already exist and will be skipped. Existing files will not be overwritten.")));
    }

    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .rounded_2xl()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_size(px(20.))
                .font_bold()
                .child("Review Clone Target Paths"),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "Cloning into project '{project_name}' at '{}' using {layout_text} structure:",
                    project_dir.display()
                )),
        )
        .child(plan_list)
        .child(
            div()
                .flex()
                .justify_end()
                .gap_3()
                .child(
                    Button::new("cancel-clone-plan")
                        .label("Back to Sources")
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| on_cancel(this, cx))),
                )
                .child(
                    Button::new("confirm-clone-plan")
                        .label("Clone selected repositories")
                        .primary()
                        .disabled(plans.is_empty())
                        .on_click(
                            cx.listener(move |this, _, window, cx| on_confirm(this, window, cx)),
                        ),
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
            list = list.child(
                div()
                    .font_bold()
                    .text_size(px(16.))
                    .child(format!("@{}", source.handle)),
            );
        }
        for repo in selected {
            count += 1;
            let display_title = if mode == RepositoryListMode::Flat {
                format!("{}/{}", source.handle, repo.name)
            } else {
                repo.name.clone()
            };

            let mut card = div()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().font_bold().text_size(px(14.)).child(display_title))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(repo.default_branch.clone()),
                        ),
                );

            if let Some(ref desc) = repo.description {
                card = card.child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().foreground)
                        .child(desc.clone()),
                );
            }

            let mut meta = div()
                .flex()
                .items_center()
                .gap_4()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground);

            if let Some(ref lang) = repo.primary_language {
                meta = meta.child(
                    div()
                        .font_bold()
                        .text_color(cx.theme().foreground)
                        .child(lang.clone()),
                );
            }
            if repo.stars > 0 {
                meta = meta.child(div().child(format!("Stars: {}", repo.stars)));
            }
            if repo.forks > 0 {
                meta = meta.child(div().child(format!("Forks: {}", repo.forks)));
            }

            card = card.child(meta);
            list = list.child(card);
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
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(20.))
                        .font_bold()
                        .child("Selected repositories"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            toggle_button(
                                "repos-grouped",
                                "Grouped",
                                mode == RepositoryListMode::Grouped,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_mode(RepositoryListMode::Grouped, this, cx)
                            })),
                        )
                        .child(
                            toggle_button("repos-flat", "Flat", mode == RepositoryListMode::Flat)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_mode(RepositoryListMode::Flat, this, cx)
                                })),
                        ),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(
                    "This display setting controls presentation within Release Desk. It is independent of the on-disk clone layout.",
                ),
        )
        .child(list)
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    fn repo() -> SourceRepository {
        SourceRepository {
            name: "PaliCAD".into(),
            full_name: "brayden/PaliCAD".into(),
            description: Some("3D modelling".into()),
            stars: 0,
            forks: 0,
            primary_language: Some("Dart".into()),
            is_private: false,
            is_fork: false,
            clone_url: String::new(),
            default_branch: String::new(),
            selected: true,
        }
    }
    #[test]
    fn search_matches_metadata_case_insensitively_and_filters_selection() {
        let repo = repo();
        assert!(matches_repository(&repo, "  PALICAD ", SourceFilter::All));
        assert!(matches_repository(&repo, "3d", SourceFilter::Selected));
        assert!(matches_repository(&repo, "dart", SourceFilter::All));
        assert!(!matches_repository(&repo, "other", SourceFilter::All));
        assert!(!matches_repository(&repo, "", SourceFilter::Unselected));
        assert!(matches_repository(&repo, "", SourceFilter::Selected));
    }
}

#[cfg(test)]
mod clone_review_tests {
    use super::{DestinationStatus, destination_status};

    #[test]
    fn destination_presence_is_reported_without_changing_files() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("repo");
        assert_eq!(
            destination_status(&destination),
            DestinationStatus::Available
        );
        std::fs::create_dir(&destination).unwrap();
        assert_eq!(destination_status(&destination), DestinationStatus::Exists);
        assert!(destination.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlink_is_not_reported_as_an_available_destination() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("repo");
        std::os::unix::fs::symlink(root.path().join("missing"), &destination).unwrap();
        assert_eq!(destination_status(&destination), DestinationStatus::Exists);
    }
}
