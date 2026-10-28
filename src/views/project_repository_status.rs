//! Repository master/detail browser using observed Git state.

use crate::components::motion_button::Button;
use crate::repository_git::RepositoryStatus;
use crate::workspace::WorkspaceProject;
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Context, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, linear_color_stop, linear_gradient,
    px,
};
use release_desk_git::CloneStatus;

fn language_color(lang: &str) -> gpui_kit::Hsla {
    match lang.to_lowercase().as_str() {
        "rust" => gpui_kit::rgb(0xDEA584).into(),
        "dart" => gpui_kit::rgb(0x00B4AB).into(),
        "python" => gpui_kit::rgb(0x3572A5).into(),
        "typescript" | "ts" => gpui_kit::rgb(0x3178C6).into(),
        "javascript" | "js" => gpui_kit::rgb(0xF7DF1E).into(),
        "go" => gpui_kit::rgb(0x00ADD8).into(),
        "c++" | "cpp" => gpui_kit::rgb(0xF34B7D).into(),
        "c" => gpui_kit::rgb(0x555555).into(),
        "html" => gpui_kit::rgb(0xE34C26).into(),
        "css" => gpui_kit::rgb(0x563D7C).into(),
        "swift" => gpui_kit::rgb(0xF05138).into(),
        "kotlin" => gpui_kit::rgb(0xA97BFF).into(),
        "java" => gpui_kit::rgb(0xB07219).into(),
        "ruby" => gpui_kit::rgb(0x701516).into(),
        "shell" | "bash" | "sh" => gpui_kit::rgb(0x89E051).into(),
        _ => gpui_kit::rgb(0x8B949E).into(),
    }
}

fn status_text(status: Option<&RepositoryStatus>) -> (&'static str, String, bool) {
    match status {
        None => (
            "Not checked",
            "Local status has not been inspected yet.".into(),
            false,
        ),
        Some(status) => match &status.state {
            Err(error) => ("Check failed", error.clone(), false),
            Ok(state) => {
                let label = match state.status {
                    CloneStatus::NotCloned => "Not cloned",
                    CloneStatus::Current if status.fetched => "Up to date",
                    CloneStatus::Current => "Locally current",
                    CloneStatus::Behind if status.fetched => "Update available",
                    CloneStatus::Behind => "Behind last fetched version",
                    CloneStatus::Ahead => "Local commits ahead",
                    CloneStatus::Diverged => "Branches diverged",
                    CloneStatus::Dirty => "Uncommitted changes",
                    CloneStatus::Detached => "Detached HEAD",
                    CloneStatus::OtherBranch => "Different branch",
                    CloneStatus::Error => "Needs attention",
                };
                let detail = state.message.clone().unwrap_or_else(|| {
                    match state.status {
                        CloneStatus::NotCloned => "No local copy exists at the project destination.".into(),
                        CloneStatus::Dirty => "Commit or stash local changes before updating this repository.".into(),
                        CloneStatus::Detached => "Check out the default branch before updating this repository.".into(),
                        CloneStatus::OtherBranch => "Switch to the default branch before updating this repository.".into(),
                        CloneStatus::Ahead => "The local branch has commits not present in the last fetched remote.".into(),
                        CloneStatus::Diverged => "Local and remote branches have different commits; resolve them in Git before updating.".into(),
                        _ if status.fetched => "Compared with the latest fetched origin.".into(),
                        _ => "Local status only; remote has not been checked.".into(),
                    }
                });
                (
                    label,
                    detail,
                    status.fetched && state.status == CloneStatus::Behind,
                )
            }
        },
    }
}

#[cfg(test)]
fn can_review_clone(status: Option<&RepositoryStatus>) -> bool {
    status.is_some_and(|status| {
        status
            .state
            .as_ref()
            .is_ok_and(|state| state.status == CloneStatus::NotCloned)
    })
}

fn status_color<T: 'static>(status: Option<&RepositoryStatus>, cx: &Context<T>) -> gpui_kit::Hsla {
    match status.and_then(|status| status.state.as_ref().ok()) {
        Some(state) => match state.status {
            CloneStatus::Current => cx.theme().success,
            CloneStatus::Behind | CloneStatus::Ahead | CloneStatus::Diverged => cx.theme().warning,
            CloneStatus::Dirty
            | CloneStatus::Detached
            | CloneStatus::OtherBranch
            | CloneStatus::Error => cx.theme().danger,
            CloneStatus::NotCloned => cx.theme().muted_foreground,
        },
        None => cx.theme().muted_foreground,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_project_repository_status<
    T: 'static,
    FCheck: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FUpdate: Fn(usize, usize, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FSelect: Fn(usize, usize, &mut T, &mut Context<T>) + Copy + 'static,
    FOpen: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
>(
    project: &WorkspaceProject,
    statuses: &[RepositoryStatus],
    selected: Option<(usize, usize)>,
    busy: bool,
    cx: &Context<T>,
    on_check: FCheck,
    on_update: FUpdate,
    on_select: FSelect,
    on_open: FOpen,
) -> impl IntoElement {
    use crate::components::ui_icons::{
        branch_icon, fork_icon, lock_icon, refresh_icon, repository_icon, star_icon,
    };

    let repositories: Vec<_> = project
        .sources
        .iter()
        .enumerate()
        .flat_map(|(source_index, source)| {
            source
                .repositories
                .iter()
                .enumerate()
                .filter(|(_, repo)| repo.selected)
                .map(move |(repo_index, repo)| (source_index, repo_index, source, repo))
        })
        .collect();
    let active = selected
        .filter(|&(s, r)| {
            repositories
                .iter()
                .any(|(si, ri, _, _)| *si == s && *ri == r)
        })
        .or_else(|| repositories.first().map(|(s, r, _, _)| (*s, *r)));
    let count_label = if repositories.len() == 1 {
        "repository"
    } else {
        "repositories"
    };

    let header = div()
        .flex()
        .items_end()
        .justify_between()
        .gap_4()
        .flex_wrap()
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
                        .child("REPOSITORY DESK"),
                )
                .child(
                    div()
                        .text_size(px(29.))
                        .font_bold()
                        .child("Your repositories"),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "{} selected {count_label} across {} sources",
                            repositories.len(),
                            project.sources.len()
                        )),
                ),
        )
        .child(
            Button::new("git-check-updates")
                .icon(refresh_icon())
                .label(if busy {
                    "Checking…"
                } else {
                    "Check for updates"
                })
                .outline()
                .disabled(busy || repositories.is_empty())
                .on_click(cx.listener(move |this, _, window, cx| on_check(this, window, cx))),
        );

    if repositories.is_empty() {
        return div().flex().flex_col().gap_5().min_h(px(540.))
            .child(header)
            .child(div().flex_1().p_6().rounded_xl().border_1().border_color(cx.theme().border)
                .flex().flex_col().items_center().justify_center().gap_3()
                                .child(div().text_size(px(20.)).font_bold().child("Choose repositories to manage"))
                .child(div().max_w(px(420.)).text_size(px(13.)).text_color(cx.theme().muted_foreground)
                    .child("Open Sources to browse the live catalogue. Select repositories there and they will appear in this workspace.")))
            .into_any_element();
    }

    let mut rail = div()
        .w(px(310.))
        .flex_shrink_0()
        .border_r_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .flex()
        .flex_col()
        .child(
            div()
                .p_4()
                .border_b_1()
                .border_color(cx.theme().border)
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_bold()
                        .child("TRACKED REPOSITORIES"),
                )
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(cx.theme().primary.opacity(0.1))
                        .text_color(cx.theme().primary)
                        .text_size(px(11.))
                        .font_bold()
                        .child(repositories.len().to_string()),
                ),
        );
    let mut rows = div()
        .flex()
        .flex_col()
        .max_h(px(650.))
        .overflow_y_scrollbar();
    for (source_index, repo_index, source, repo) in &repositories {
        let (source_index, repo_index) = (*source_index, *repo_index);
        let status = statuses
            .iter()
            .find(|status| status.source_index == source_index && status.repo_index == repo_index);
        let (label, _, _) = status_text(status);
        let is_active = active == Some((source_index, repo_index));
        let owner = crate::repository_git::repository_owner(source, repo);
        let color = status_color(status, cx);
        let row = div()
            .id(ElementId::Name(
                format!("repo-row-{source_index}-{repo_index}").into(),
            ))
            .flex()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                on_select(source_index, repo_index, this, cx);
            }))
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(if is_active {
                cx.theme().background
            } else {
                cx.theme().secondary.opacity(0.1)
            })
            .child(div().w(px(3.)).flex_shrink_0().bg(if is_active {
                cx.theme().primary
            } else {
                cx.theme().secondary.opacity(0.1)
            }))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .child(
                                Button::new(format!("repository-{source_index}-{repo_index}"))
                                    .icon(crate::components::provider_icon::provider_icon(
                                        crate::workspace::ProviderChoice::from_source_provider(
                                            &source.provider,
                                        ),
                                    ))
                                    .label(repo.name.clone())
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        on_select(source_index, repo_index, this, cx)
                                    })),
                            )
                            .child(
                                div().min_w_0().flex().flex_col().child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(owner.to_string()),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .line_clamp(2)
                            .child(
                                repo.description
                                    .clone()
                                    .filter(|description| !description.is_empty())
                                    .unwrap_or_else(|| "No description provided".into()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(11.))
                            .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(color))
                            .child(div().text_color(color).child(label))
                            .child(div().text_color(cx.theme().muted_foreground).child("·"))
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(source.provider.clone()),
                            ),
                    ),
            );
        rows = rows.child(row);
    }
    rail = rail.child(rows);

    let Some((source_index, repo_index, source, repo)) = repositories
        .into_iter()
        .find(|(s, r, _, _)| Some((*s, *r)) == active)
    else {
        return div().child(header).child(rail).into_any_element();
    };
    let status = statuses
        .iter()
        .find(|item| item.source_index == source_index && item.repo_index == repo_index);
    let (label, detail, can_update) = status_text(status);
    let status_color = status_color(status, cx);
    let branch = if repo.default_branch.is_empty() {
        "Unknown"
    } else {
        &repo.default_branch
    };
    let provider = crate::workspace::ProviderChoice::from_source_provider(&source.provider);
    let browser_url = source.repository_browser_url(repo).or_else(|| {
        Some(format!(
            "https://github.com/{}/{}",
            crate::repository_git::repository_owner(source, repo),
            repo.name
        ))
    });
    let clone_path = project.disk_layout.compute_clone_path(
        &project.directory,
        crate::repository_git::repository_owner(source, repo),
        &repo.name,
    );
    let is_cloned = std::fs::symlink_metadata(&clone_path).is_ok();
    let checked_out_branch = status
        .and_then(|item| item.state.as_ref().ok())
        .and_then(|state| state.branch.as_deref())
        .unwrap_or(if is_cloned {
            "Default / detached"
        } else {
            "Not cloned locally"
        });

    let repo_avatar = if let Some((url, path)) = source.avatar_cache_request() {
        if path.is_file() {
            div()
                .relative()
                .child(
                    gpui_kit::img(path)
                        .w(px(56.))
                        .h(px(56.))
                        .rounded_xl()
                        .border_1()
                        .border_color(cx.theme().border),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(-3.))
                        .right(px(-3.))
                        .p_1()
                        .rounded_md()
                        .bg(cx.theme().background)
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(
                            crate::components::provider_icon::provider_icon(provider)
                                .w(px(14.))
                                .h(px(14.)),
                        ),
                )
                .into_any_element()
        } else {
            let path_clone = path.clone();
            std::thread::spawn(move || {
                let _ = release_desk_store::source_fetcher::cache_avatar(&url, &path_clone);
            });
            div()
                .relative()
                .w(px(56.))
                .h(px(56.))
                .rounded_xl()
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(cx.theme().primary, 0.),
                    linear_color_stop(cx.theme().primary.opacity(0.6), 1.),
                ))
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(22.))
                        .font_bold()
                        .text_color(gpui_kit::white())
                        .child(
                            repo.name
                                .chars()
                                .next()
                                .unwrap_or('R')
                                .to_uppercase()
                                .to_string(),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(-3.))
                        .right(px(-3.))
                        .p_1()
                        .rounded_md()
                        .bg(cx.theme().background)
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(
                            crate::components::provider_icon::provider_icon(provider)
                                .w(px(12.))
                                .h(px(12.)),
                        ),
                )
                .into_any_element()
        }
    } else {
        div()
            .relative()
            .w(px(56.))
            .h(px(56.))
            .rounded_xl()
            .bg(linear_gradient(
                135.,
                linear_color_stop(cx.theme().primary, 0.),
                linear_color_stop(cx.theme().primary.opacity(0.6), 1.),
            ))
            .border_1()
            .border_color(cx.theme().border)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(22.))
                    .font_bold()
                    .text_color(gpui_kit::white())
                    .child(
                        repo.name
                            .chars()
                            .next()
                            .unwrap_or('R')
                            .to_uppercase()
                            .to_string(),
                    ),
            )
            .into_any_element()
    };

    let mut chips = div().flex().flex_wrap().gap_2();
    if let Some(ref lang) = repo.primary_language {
        chips = chips.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(cx.theme().secondary)
                .child(
                    div()
                        .w(px(8.))
                        .h(px(8.))
                        .rounded_full()
                        .bg(language_color(lang)),
                )
                .child(div().text_size(px(12.)).font_medium().child(lang.clone())),
        );
    }
    chips = chips
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(cx.theme().secondary)
                .child(star_icon())
                .child(div().text_size(px(12.)).child(repo.stars.to_string())),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(cx.theme().secondary)
                .child(fork_icon())
                .child(div().text_size(px(12.)).child(repo.forks.to_string())),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(cx.theme().secondary)
                .child(if repo.is_private {
                    lock_icon().into_any_element()
                } else {
                    repository_icon().into_any_element()
                })
                .child(div().text_size(px(12.)).child(if repo.is_private {
                    "Private"
                } else {
                    "Public"
                })),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(cx.theme().secondary)
                .child(branch_icon())
                .child(div().text_size(px(12.)).child(format!("Default: {branch}"))),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(cx.theme().secondary)
                .child(branch_icon())
                .child(
                    div()
                        .text_size(px(12.))
                        .child(format!("Local branch: {checked_out_branch}")),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(status_color.opacity(0.15))
                .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(status_color))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(status_color)
                        .font_medium()
                        .child(label),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(cx.theme().secondary)
                .child(crate::components::ui_icons::folder_icon())
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(clone_path.display().to_string()),
                ),
        );
    if repo.is_fork {
        chips = chips.child(
            div()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(cx.theme().secondary)
                .text_size(px(12.))
                .child("Fork"),
        );
    }

    let repo_metadata_card = div()
        .p_5()
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.opacity(0.35))
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_size(px(11.))
                .font_bold()
                .text_color(cx.theme().primary)
                .child("REPOSITORY INFORMATION"),
        )
        .child(
            div()
                .grid()
                .grid_cols(2)
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Default branch"),
                        )
                        .child(
                            div()
                                .font_bold()
                                .text_size(px(13.))
                                .child(branch.to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Checked-out branch"),
                        )
                        .child(
                            div()
                                .font_bold()
                                .text_size(px(13.))
                                .child(checked_out_branch.to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Remote clone URL"),
                        )
                        .child(
                            div()
                                .font_medium()
                                .text_size(px(12.))
                                .text_color(cx.theme().foreground)
                                .child(if repo.clone_url.is_empty() {
                                    "Not available".to_string()
                                } else {
                                    repo.clone_url.clone()
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Clone destination on disk"),
                        )
                        .child(
                            div()
                                .font_medium()
                                .text_size(px(12.))
                                .text_color(cx.theme().foreground)
                                .child(clone_path.display().to_string()),
                        ),
                ),
        );

    let git_status_card = div()
        .p_5()
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary.opacity(0.35))
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_size(px(11.))
                .font_bold()
                .text_color(cx.theme().muted_foreground)
                .child("LOCAL GIT STATUS"),
        )
        .child(
            div()
                .rounded_lg()
                .border_1()
                .border_color(status_color.opacity(0.4))
                .bg(status_color.opacity(0.08))
                .flex()
                .items_start()
                .gap_3()
                .p_4()
                .child(
                    div()
                        .w(px(8.))
                        .h(px(8.))
                        .mt_1()
                        .rounded_full()
                        .bg(status_color),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_bold().text_color(status_color).child(label))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(detail),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .child(branch_icon())
                .child(div().text_size(px(11.)).child(
                    if status.is_some_and(|item| item.fetched) {
                        "Remote comparison completed for this repository."
                    } else {
                        "Remote changes have not been checked in this session."
                    },
                )),
        );

    let mut detail_panel = div()
        .flex_1()
        .min_w(px(320.))
        .bg(cx.theme().background)
        .flex()
        .flex_col()
        .child(
            div()
                .p_6()
                .flex()
                .flex_col()
                .gap_5()
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_4()
                                .child(repo_avatar)
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!(
                                                    "{} / {}",
                                                    crate::repository_git::repository_owner(
                                                        source, repo
                                                    ),
                                                    source.provider
                                                )),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(28.))
                                                .font_bold()
                                                .child(repo.name.clone()),
                                        ),
                                ),
                        )
                        .child(
                            Button::new("open-repository-provider")
                                .icon(crate::components::provider_icon::provider_icon(provider))
                                .label(format!("Open on {}", provider.label()))
                                .primary()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(url) = &browser_url {
                                        on_open(url.clone(), this, cx);
                                    }
                                })),
                        ),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            repo.description
                                .clone()
                                .filter(|description| !description.is_empty())
                                .unwrap_or_else(|| {
                                    "This repository has no description yet.".into()
                                }),
                        ),
                )
                .child(chips)
                .child(repo_metadata_card)
                .child(git_status_card),
        );
    if can_update && !busy {
        detail_panel = detail_panel.child(
            div().px_6().pb_6().child(
                Button::new("git-update-selected")
                    .label("Update repository")
                    .primary()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        on_update(source_index, repo_index, this, window, cx)
                    })),
            ),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap_5()
        .child(header)
        .child(
            div()
                .flex()
                .flex_wrap()
                .border_t_1()
                .border_color(cx.theme().border)
                .items_start()
                .child(rail)
                .child(detail_panel),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use release_desk_git::{CloneState, CloneStatus};

    fn status(state: CloneStatus, fetched: bool) -> RepositoryStatus {
        RepositoryStatus {
            source_index: 0,
            repo_index: 0,
            state: Ok(CloneState {
                status: state,
                branch: None,
                local_revision: None,
                remote_revision: None,
                message: None,
            }),
            fetched,
        }
    }

    #[test]
    fn update_action_requires_fresh_check_and_behind_state() {
        assert!(!status_text(None).2);
        assert!(!status_text(Some(&status(CloneStatus::Behind, false))).2);
        assert!(status_text(Some(&status(CloneStatus::Behind, true))).2);
        assert!(!status_text(Some(&status(CloneStatus::Dirty, true))).2);
        assert!(!status_text(Some(&status(CloneStatus::Current, true))).2);
    }

    #[test]
    fn clone_review_requires_observed_missing_clone() {
        assert!(!can_review_clone(None));
        assert!(can_review_clone(Some(&status(
            CloneStatus::NotCloned,
            false
        ))));
        assert!(!can_review_clone(Some(&status(CloneStatus::Current, true))));
        assert!(!can_review_clone(Some(&status(CloneStatus::Error, false))));
    }

    #[test]
    fn local_and_fetched_states_have_distinct_labels() {
        assert_eq!(
            status_text(Some(&status(CloneStatus::Current, false))).0,
            "Locally current"
        );
        assert_eq!(
            status_text(Some(&status(CloneStatus::Current, true))).0,
            "Up to date"
        );
        assert_eq!(
            status_text(Some(&status(CloneStatus::Behind, false))).0,
            "Behind last fetched version"
        );
    }
}
