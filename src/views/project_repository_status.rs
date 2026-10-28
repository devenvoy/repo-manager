//! Repository master/detail browser matching the ArtCraft Release Desk design.

use crate::components::app_icon::repo_icon;
use crate::components::avatar::source_avatar;
use crate::components::badge::{branch_chip, language_chip, version_pill, visibility_chip};
use crate::components::motion_button::Button;
use crate::components::ui_icons::{
    alert_circle_icon, android_icon, check_icon, chevron_down_icon, chevron_right_icon, code_icon,
    download_icon, external_link_icon, folder_icon, git_commit_icon, hammer_icon, refresh_icon,
    terminal_icon,
};
use crate::repository_git::{RepositoryStatus, repository_owner};
use crate::styles::CssExt as _;
use crate::workspace::WorkspaceProject;
use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    Context, ElementId, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, deferred, div, px,
};
use release_desk_core::app_catalog::find_known_app;
use release_desk_git::CloneStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorIconKind {
    VsCode,
    Cursor,
    Xcode,
    AndroidStudio,
    Zed,
    Terminal,
    Finder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalEditor {
    pub id: &'static str,
    pub name: &'static str,
    pub app_name: &'static str,
    pub icon_kind: EditorIconKind,
}

pub fn is_app_installed(app_name: &str) -> bool {
    if app_name == "Finder" || app_name == "Terminal" {
        return true;
    }
    let paths = [
        format!("/Applications/{app_name}.app"),
        format!("/Applications/Utilities/{app_name}.app"),
        format!("/System/Applications/{app_name}.app"),
        format!("/System/Applications/Utilities/{app_name}.app"),
    ];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return true;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let user_app = format!("{home}/Applications/{app_name}.app");
        if std::path::Path::new(&user_app).exists() {
            return true;
        }
    }
    false
}

pub fn available_editors() -> Vec<ExternalEditor> {
    let all = [
        ExternalEditor {
            id: "vscode",
            name: "Visual Studio Code",
            app_name: "Visual Studio Code",
            icon_kind: EditorIconKind::VsCode,
        },
        ExternalEditor {
            id: "cursor",
            name: "Cursor",
            app_name: "Cursor",
            icon_kind: EditorIconKind::Cursor,
        },
        ExternalEditor {
            id: "xcode",
            name: "Xcode",
            app_name: "Xcode",
            icon_kind: EditorIconKind::Xcode,
        },
        ExternalEditor {
            id: "android-studio",
            name: "Android Studio",
            app_name: "Android Studio",
            icon_kind: EditorIconKind::AndroidStudio,
        },
        ExternalEditor {
            id: "zed",
            name: "Zed",
            app_name: "Zed",
            icon_kind: EditorIconKind::Zed,
        },
        ExternalEditor {
            id: "iterm",
            name: "iTerm",
            app_name: "iTerm",
            icon_kind: EditorIconKind::Terminal,
        },
        ExternalEditor {
            id: "terminal",
            name: "Terminal",
            app_name: "Terminal",
            icon_kind: EditorIconKind::Terminal,
        },
        ExternalEditor {
            id: "finder",
            name: "Finder",
            app_name: "Finder",
            icon_kind: EditorIconKind::Finder,
        },
    ];

    let installed: Vec<_> = all
        .into_iter()
        .filter(|e| is_app_installed(e.app_name))
        .collect();

    if installed.is_empty() {
        vec![
            ExternalEditor {
                id: "finder",
                name: "Finder",
                app_name: "Finder",
                icon_kind: EditorIconKind::Finder,
            },
            ExternalEditor {
                id: "terminal",
                name: "Terminal",
                app_name: "Terminal",
                icon_kind: EditorIconKind::Terminal,
            },
        ]
    } else {
        installed
    }
}

pub fn open_path_in_editor(path: &std::path::Path, editor_id: &str) {
    let editors = available_editors();
    let editor = editors.iter().find(|e| e.id == editor_id);
    let app_name = editor.map(|e| e.app_name).unwrap_or(editor_id);

    if app_name == "Finder" {
        let _ = std::process::Command::new("open").arg(path).spawn();
    } else {
        let _ = std::process::Command::new("open")
            .args(["-a", app_name])
            .arg(path)
            .spawn();
    }
}

fn editor_icon(kind: EditorIconKind) -> gpui_kit::component::Icon {
    match kind {
        EditorIconKind::VsCode | EditorIconKind::Cursor | EditorIconKind::Zed => code_icon(),
        EditorIconKind::Xcode => hammer_icon(),
        EditorIconKind::AndroidStudio => android_icon(),
        EditorIconKind::Terminal => terminal_icon(),
        EditorIconKind::Finder => folder_icon(),
    }
}

fn format_date_short(raw: &str) -> String {
    if raw.len() >= 10 {
        let parts: Vec<&str> = raw[..10].split('-').collect();
        if parts.len() == 3 {
            let month = match parts[1] {
                "01" => "Jan",
                "02" => "Feb",
                "03" => "Mar",
                "04" => "Apr",
                "05" => "May",
                "06" => "Jun",
                "07" => "Jul",
                "08" => "Aug",
                "09" => "Sep",
                "10" => "Oct",
                "11" => "Nov",
                "12" => "Dec",
                _ => parts[1],
            };
            return format!("{month} {}, {}", parts[2].trim_start_matches('0'), parts[0]);
        }
    }
    raw.to_string()
}

pub(crate) fn status_text(status: Option<&RepositoryStatus>) -> (&'static str, String, bool) {
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
                        CloneStatus::NotCloned => {
                            "No local copy exists at the project destination.".into()
                        }
                        CloneStatus::Dirty => {
                            "Commit or stash local changes before updating this repository.".into()
                        }
                        CloneStatus::Detached => {
                            "Check out the default branch before updating this repository.".into()
                        }
                        CloneStatus::OtherBranch => {
                            "Switch to the default branch before updating this repository.".into()
                        }
                        CloneStatus::Ahead => {
                            "The local branch has commits not present in the last fetched remote."
                                .into()
                        }
                        CloneStatus::Diverged => {
                            "Local and remote branches have different commits; resolve them in Git before updating."
                                .into()
                        }
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
pub(crate) fn can_review_clone(status: Option<&RepositoryStatus>) -> bool {
    status.is_some_and(|status| {
        status
            .state
            .as_ref()
            .is_ok_and(|state| state.status == CloneStatus::NotCloned)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn render_project_repository_status<
    T: 'static,
    FCheck: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FUpdate: Fn(usize, usize, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FClone: Fn(usize, usize, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FCloneAll: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FSelect: Fn(usize, usize, &mut T, &mut Context<T>) + Copy + 'static,
    FOpen: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FBrowseSources: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FToggleCommits: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FToggleEditorMenu: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FSelectEditor: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FOpenInEditor: Fn(std::path::PathBuf, String, &mut T, &mut Context<T>) + Copy + 'static,
>(
    project: &WorkspaceProject,
    statuses: &[RepositoryStatus],
    selected: Option<(usize, usize)>,
    search_input: &Entity<InputState>,
    busy: bool,
    commits_collapsed: bool,
    selected_editor_id: &str,
    editor_menu_open: bool,
    cx: &Context<T>,
    on_check: FCheck,
    on_update: FUpdate,
    on_clone: FClone,
    on_clone_all: FCloneAll,
    on_select: FSelect,
    on_open: FOpen,
    on_browse_sources: FBrowseSources,
    on_toggle_commits: FToggleCommits,
    on_toggle_editor_menu: FToggleEditorMenu,
    on_select_editor: FSelectEditor,
    on_open_in_editor: FOpenInEditor,
) -> impl IntoElement {
    let all_repositories: Vec<_> = project
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

    let query = search_input.read(cx).value().trim().to_lowercase();
    let repositories: Vec<_> = all_repositories
        .iter()
        .copied()
        .filter(|(_, _, _, repo)| {
            if query.is_empty() {
                true
            } else {
                repo.name.to_lowercase().contains(&query)
                    || repo
                        .description
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&query))
            }
        })
        .collect();

    let active = selected
        .filter(|&(s, r)| {
            repositories
                .iter()
                .any(|(si, ri, _, _)| *si == s && *ri == r)
        })
        .or_else(|| repositories.first().map(|(s, r, _, _)| (*s, *r)));

    let uncloned_count = all_repositories
        .iter()
        .filter(|(s_idx, r_idx, s, r)| {
            let status = statuses
                .iter()
                .find(|item| item.source_index == *s_idx && item.repo_index == *r_idx);
            let not_cloned_status = status
                .and_then(|item| item.state.as_ref().ok())
                .is_some_and(|st| st.status == CloneStatus::NotCloned);
            let owner = crate::repository_git::repository_owner(s, r);
            let target_path = crate::repository_git::resolve_target_path(
                &project.directory,
                project.disk_layout,
                owner,
                &r.name,
            );
            not_cloned_status || !target_path.exists()
        })
        .count();

    let mut header_actions = div().flex().items_center().gap_3();

    if uncloned_count > 0 {
        header_actions = header_actions.child(
            Button::new("git-clone-all-missing")
                .icon(download_icon())
                .label(format!("Clone all missing ({uncloned_count})"))
                .primary()
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| on_clone_all(this, window, cx))),
        );
    }

    header_actions = header_actions.child(
        Button::new("git-check-updates")
            .icon(refresh_icon())
            .label(if busy {
                "Checking…"
            } else {
                "Check for updates"
            })
            .outline()
            .disabled(busy || all_repositories.is_empty())
            .on_click(cx.listener(move |this, _, window, cx| on_check(this, window, cx))),
    );

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
                        .css_section_eyebrow(cx.theme())
                        .child("REPOSITORY DESK"),
                )
                .child(div().css_heading_title().child("Your repositories"))
                .child(div().css_body_muted(cx.theme()).child(format!(
                    "{} selected repositories across {} sources",
                    all_repositories.len(),
                    project.sources.len()
                ))),
        )
        .child(header_actions);

    if all_repositories.is_empty() {
        return div()
            .flex()
            .flex_col()
            .gap_5()
            .min_h(px(540.))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .p_8()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_bold()
                            .child("Choose repositories to manage"),
                    )
                    .child(
                        div()
                            .max_w(px(460.))
                            .text_size(px(13.))
                            .text_color(cx.theme().muted_foreground)
                            .text_center()
                            .child(
                                "Select repositories from remote sources, or scan your project folder in Settings to import existing local Git repositories.",
                            ),
                    )
                    .child(
                        Button::new("empty-browse-sources")
                            .label("Browse sources")
                            .primary()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_browse_sources(this, window, cx)
                            })),
                    ),
            )
            .into_any_element();
    }

    // Sidebar Rail
    let rail_header = div()
        .p_3p5()
        .border_b_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_col()
        .gap_2p5()
        .child(Input::new(search_input))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_bold()
                        .text_color(cx.theme().muted_foreground)
                        .child("TRACKED REPOSITORIES"),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(cx.theme().primary.opacity(0.1))
                        .text_color(cx.theme().primary)
                        .text_size(px(10.))
                        .font_bold()
                        .child(format!("{} items", repositories.len())),
                ),
        );

    let mut rows = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scrollbar();

    for (source_index, repo_index, source, repo) in &repositories {
        let (source_index, repo_index) = (*source_index, *repo_index);
        let status = statuses
            .iter()
            .find(|status| status.source_index == source_index && status.repo_index == repo_index);

        let is_active = active == Some((source_index, repo_index));
        let known = find_known_app(&repo.name);

        let is_current = status
            .and_then(|s| s.state.as_ref().ok())
            .is_some_and(|st| matches!(st.status, CloneStatus::Current));

        let is_fetched = status.is_some_and(|s| s.fetched);

        let is_not_cloned = status
            .and_then(|s| s.state.as_ref().ok())
            .is_some_and(|st| st.status == CloneStatus::NotCloned);

        let is_behind = status
            .and_then(|s| s.state.as_ref().ok())
            .is_some_and(|st| st.status == CloneStatus::Behind);

        let is_error = status.is_some_and(|s| {
            s.state.is_err()
                || s.state
                    .as_ref()
                    .ok()
                    .is_some_and(|st| st.status == CloneStatus::Error)
        });

        let status_badge = if is_not_cloned {
            div()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(cx.theme().secondary.opacity(0.15))
                .text_color(cx.theme().muted_foreground)
                .text_size(px(10.))
                .font_bold()
                .child("Not cloned")
        } else if is_behind {
            div()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(cx.theme().warning.opacity(0.12))
                .text_color(cx.theme().warning)
                .text_size(px(10.))
                .font_bold()
                .flex()
                .items_center()
                .gap_1()
                .child(download_icon().w(px(10.)).h(px(10.)))
                .child("Update available")
        } else if is_current && is_fetched {
            div()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(cx.theme().success.opacity(0.12))
                .text_color(cx.theme().success)
                .text_size(px(10.))
                .font_bold()
                .flex()
                .items_center()
                .gap_1()
                .child(check_icon().w(px(10.)).h(px(10.)))
                .child("Up to date")
        } else if is_current {
            div()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(cx.theme().primary.opacity(0.12))
                .text_color(cx.theme().primary)
                .text_size(px(10.))
                .font_bold()
                .flex()
                .items_center()
                .gap_1()
                .child(check_icon().w(px(10.)).h(px(10.)))
                .child("Code current")
        } else if is_error {
            div()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(cx.theme().danger.opacity(0.12))
                .text_color(cx.theme().danger)
                .text_size(px(10.))
                .font_bold()
                .flex()
                .items_center()
                .gap_1()
                .child(alert_circle_icon().w(px(10.)).h(px(10.)))
                .child("Error")
        } else {
            let (label, _, _) = status_text(status);
            div()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(cx.theme().secondary.opacity(0.15))
                .text_color(cx.theme().muted_foreground)
                .text_size(px(10.))
                .font_bold()
                .child(label)
        };

        let repo_dir = crate::repository_git::resolve_target_path(
            &project.directory,
            project.disk_layout,
            repository_owner(source, repo),
            &repo.name,
        );
        let row_icon_path = crate::components::app_icon::find_local_repo_icon(
            &repo_dir, &repo.name,
        )
        .or_else(|| {
            crate::components::app_icon::find_cached_repo_icon(
                &repo.name,
                Some(repository_owner(source, repo)),
            )
        });

        let snapshot_rels = release_desk_store::workspace_releases::load_snapshot_releases(
            &project.directory,
            &repo.name,
        );
        let version_tag = snapshot_rels
            .as_ref()
            .and_then(|r| r.first())
            .map(|r| r.tag_name.clone());

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
                cx.theme().primary.opacity(0.08)
            } else {
                cx.theme().background
            })
            .child(div().w(px(3.)).flex_shrink_0().bg(if is_active {
                cx.theme().primary
            } else {
                gpui_kit::transparent_black()
            }))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .p_3p5()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2p5()
                            .child(repo_icon(&repo.name, 36., row_icon_path.as_deref()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .font_bold()
                                            .text_size(px(14.))
                                            .text_color(if is_active {
                                                cx.theme().primary
                                            } else {
                                                cx.theme().foreground
                                            })
                                            .truncate()
                                            .child(repo.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .truncate()
                                            .child(source.handle.clone()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .line_clamp(2)
                            .child(
                                repo.description
                                    .clone()
                                    .filter(|d| !d.is_empty())
                                    .or_else(|| known.as_ref().map(|k| k.description.clone()))
                                    .unwrap_or_else(|| "Local Git workspace component".into()),
                            ),
                    )
                    .child({
                        let mut tags = div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .flex_wrap()
                            .child(status_badge);
                        if let Some(ref ver) = version_tag {
                            tags = tags.child(version_pill(ver, cx.theme()));
                        }
                        tags.child(branch_chip(&repo.default_branch, cx.theme()))
                    }),
            );
        rows = rows.child(row);
    }

    let rail = div()
        .w(px(330.))
        .flex_shrink_0()
        .border_r_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .flex()
        .flex_col()
        .min_h_0()
        .h(px(640.))
        .child(rail_header)
        .child(rows);

    // Detail Pane
    let active_entry = active.and_then(|(source_index, repo_index)| {
        all_repositories
            .iter()
            .find(|(si, ri, _, _)| *si == source_index && *ri == repo_index)
            .copied()
    });

    let detail_panel = if let Some((source_index, repo_index, source, repo)) = active_entry {
        let status = statuses
            .iter()
            .find(|status| status.source_index == source_index && status.repo_index == repo_index);

        let owner = crate::repository_git::repository_owner(source, repo);
        let target_path = crate::repository_git::resolve_target_path(
            &project.directory,
            project.disk_layout,
            owner,
            &repo.name,
        );

        let is_cloned = target_path.exists()
            && !status
                .and_then(|item| item.state.as_ref().ok())
                .is_some_and(|st| st.status == CloneStatus::NotCloned);

        let known = find_known_app(&repo.name);
        let browser_url = source
            .repository_browser_url(repo)
            .unwrap_or_else(|| repo.clone_url.clone());

        let app_title = known
            .as_ref()
            .map(|k| k.name.clone())
            .unwrap_or_else(|| repo.name.clone());
        let app_desc = repo
            .description
            .clone()
            .filter(|d| !d.is_empty())
            .or_else(|| known.as_ref().map(|k| k.description.clone()))
            .unwrap_or_else(|| "Local Git workspace component".into());

        let detail_icon_path =
            crate::components::app_icon::find_local_repo_icon(&target_path, &repo.name).or_else(
                || crate::components::app_icon::find_cached_repo_icon(&repo.name, Some(owner)),
            );

        let mut meta_badges = div().flex().items_center().gap_2p5().pt_2().flex_wrap();
        if let Some(ref lang) = repo.primary_language {
            meta_badges = meta_badges.child(language_chip(lang, cx.theme()));
        }
        meta_badges = meta_badges.child(visibility_chip(repo.is_private, cx.theme()));
        meta_badges = meta_badges.child(branch_chip(&repo.default_branch, cx.theme()));

        // Hero Header matching reference
        let hero = div()
            .flex()
            .items_start()
            .justify_between()
            .gap_4()
            .flex_wrap()
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_4()
                    .child(repo_icon(&repo.name, 60., detail_icon_path.as_deref()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(source_avatar(
                                        &source.handle,
                                        source.avatar_cache_request().map(|(_, p)| p).as_deref(),
                                        18.,
                                        cx.theme(),
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_bold()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(owner.to_uppercase()),
                                    ),
                            )
                            .child(div().font_bold().text_size(px(24.)).child(app_title))
                            .child(
                                div()
                                    .max_w(px(640.))
                                    .text_size(px(13.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(app_desc),
                            )
                            .child(meta_badges),
                    ),
            )
            .child(
                Button::new("open-github-action")
                    .icon(external_link_icon())
                    .label("Open on GitHub")
                    .outline()
                    .on_click(cx.listener({
                        let url = browser_url.clone();
                        move |this, _, _, cx| on_open(url.clone(), this, cx)
                    })),
            );

        // Horizontal directory row with installed IDE dropdown and Open button
        let editors = available_editors();
        let current_editor = editors
            .iter()
            .find(|e| e.id == selected_editor_id)
            .or_else(|| editors.first())
            .cloned()
            .unwrap_or(ExternalEditor {
                id: "finder",
                name: "Finder",
                app_name: "Finder",
                icon_kind: EditorIconKind::Finder,
            });

        let mut menu_items = div().flex().flex_col().gap_0p5();
        for editor in &editors {
            let is_selected = editor.id == current_editor.id;
            let ed_id = editor.id.to_string();
            let mut item = div()
                .id(ElementId::Name(
                    format!("select-editor-{}", editor.id).into(),
                ))
                .flex()
                .items_center()
                .justify_between()
                .px_2p5()
                .py_1p5()
                .rounded_md()
                .cursor_pointer()
                .bg(if is_selected {
                    cx.theme().primary.opacity(0.12)
                } else {
                    cx.theme().background
                })
                .hover(|s| s.bg(cx.theme().secondary.opacity(0.2)))
                .on_click(cx.listener({
                    let ed_id = ed_id.clone();
                    move |this, _, _, cx| {
                        on_select_editor(ed_id.clone(), this, cx);
                    }
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            editor_icon(editor.icon_kind)
                                .w(px(14.))
                                .h(px(14.))
                                .text_color(if is_selected {
                                    cx.theme().primary
                                } else {
                                    cx.theme().foreground
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_medium()
                                .text_color(if is_selected {
                                    cx.theme().primary
                                } else {
                                    cx.theme().foreground
                                })
                                .child(editor.name),
                        ),
                );

            if is_selected {
                item = item.child(
                    check_icon()
                        .w(px(13.))
                        .h(px(13.))
                        .text_color(cx.theme().primary),
                );
            }

            menu_items = menu_items.child(item);
        }

        let mut selector_container = div().relative().child(
            div()
                .id("editor-select-trigger")
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1p5()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.12))
                .hover(|s| s.bg(cx.theme().secondary.opacity(0.25)))
                .cursor_pointer()
                .child(
                    editor_icon(current_editor.icon_kind)
                        .w(px(14.))
                        .h(px(14.))
                        .text_color(cx.theme().foreground),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_medium()
                        .text_color(cx.theme().foreground)
                        .child(current_editor.name),
                )
                .child(
                    chevron_down_icon()
                        .w(px(12.))
                        .h(px(12.))
                        .text_color(cx.theme().muted_foreground),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    on_toggle_editor_menu(this, cx);
                })),
        );

        if editor_menu_open {
            selector_container = selector_container.child(
                deferred(
                    div()
                        .child(
                            div()
                                .id("editor-menu-backdrop")
                                .occlude()
                                .absolute()
                                .top(px(-1000.))
                                .left(px(-2000.))
                                .w(px(5000.))
                                .h(px(5000.))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_toggle_editor_menu(this, cx);
                                })),
                        )
                        .child(
                            div()
                                .occlude()
                                .absolute()
                                .top_full()
                                .right_0()
                                .mt_1p5()
                                .w(px(220.))
                                .rounded_xl()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().background)
                                .shadow_lg()
                                .p_1p5()
                                .child(menu_items),
                        ),
                )
                .priority(10),
            );
        }

        let directory_row = div()
            .flex()
            .items_center()
            .justify_between()
            .py_2p5()
            .px_1()
            .border_t_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .min_w_0()
                    .child(
                        folder_icon()
                            .w(px(16.))
                            .h(px(16.))
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_bold()
                            .text_color(cx.theme().muted_foreground)
                            .child("Directory:"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_family(".AppleSystemUIFontMonospaced, Menlo, monospace")
                            .text_color(cx.theme().foreground)
                            .truncate()
                            .child(target_path.display().to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_shrink_0()
                    .child(selector_container)
                    .child(
                        Button::new("open-directory-in-app")
                            .icon(external_link_icon())
                            .label("Open")
                            .primary()
                            .on_click(cx.listener({
                                let path = target_path.clone();
                                let ed_id = current_editor.id.to_string();
                                move |this, _, _, cx| {
                                    on_open_in_editor(path.clone(), ed_id.clone(), this, cx);
                                }
                            })),
                    ),
            );

        // Git Action Card
        let git_card = if !is_cloned {
            div()
                .p_4()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().warning.opacity(0.3))
                .bg(cx.theme().warning.opacity(0.05))
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
                                .font_bold()
                                .text_size(px(14.))
                                .child("Repository not cloned locally"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Clone this repository to enable local builds, commits, and branch switching."),
                        ),
                )
                .child(
                    Button::new("git-clone-selected")
                        .icon(download_icon())
                        .label(if busy { "Cloning…" } else { "Clone repository" })
                        .primary()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_clone(source_index, repo_index, this, window, cx)
                        })),
                )
        } else {
            let (status_title, status_desc, is_behind, is_error) = match status {
                None => (
                    "Not checked",
                    "Local repository status has not been inspected yet.".to_string(),
                    false,
                    false,
                ),
                Some(st) => match &st.state {
                    Err(err) => (
                        "Update failed",
                        format!("Error updating repository: {err}"),
                        false,
                        true,
                    ),
                    Ok(state) => match state.status {
                        CloneStatus::NotCloned => (
                            "Not cloned",
                            "No local copy exists at the project destination.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Current if st.fetched => (
                            "Up to date",
                            "Git working tree is in sync with latest tracked upstream commits.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Current => (
                            "Locally current",
                            "Working tree is clean. Fetch origin to check if remote updates are available.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Behind => (
                            "Update available",
                            state.message.clone().unwrap_or_else(|| {
                                "New upstream commits are available. Pull updates to bring your local repository up to date.".to_string()
                            }),
                            true,
                            false,
                        ),
                        CloneStatus::Ahead => (
                            "Local commits ahead",
                            "The local branch has commits not present in upstream remote.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Diverged => (
                            "Branches diverged",
                            "Local and remote branches have diverged; manual resolution required in Git.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Dirty => (
                            "Uncommitted changes",
                            "Commit or stash local modifications before pulling updates.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Detached => (
                            "Detached HEAD",
                            "Check out the default branch before pulling updates.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::OtherBranch => (
                            "Different branch",
                            "Switch to the default branch before pulling updates.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Error => (
                            "Update error",
                            state.message.clone().unwrap_or_else(|| {
                                "Repository update failed or requires manual resolution in Git.".to_string()
                            }),
                            false,
                            true,
                        ),
                    },
                },
            };

            let card_bg = if is_error {
                cx.theme().danger.opacity(0.06)
            } else if is_behind {
                cx.theme().primary.opacity(0.05)
            } else {
                cx.theme().background
            };

            let card_border = if is_error {
                cx.theme().danger.opacity(0.35)
            } else if is_behind {
                cx.theme().primary.opacity(0.3)
            } else {
                cx.theme().border
            };

            let title_color = if is_error {
                cx.theme().danger
            } else if is_behind {
                cx.theme().primary
            } else {
                cx.theme().foreground
            };

            let info_col = div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_bold()
                        .text_size(px(14.))
                        .text_color(title_color)
                        .child(status_title),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if is_error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(status_desc),
                );

            let action_btn = if is_behind {
                Button::new("git-update-selected")
                    .icon(download_icon())
                    .label(if busy {
                        "Pulling updates…"
                    } else {
                        "Pull updates"
                    })
                    .primary()
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        on_update(source_index, repo_index, this, window, cx)
                    }))
            } else if is_error {
                Button::new("git-update-selected")
                    .icon(refresh_icon())
                    .label(if busy { "Retrying…" } else { "Retry update" })
                    .danger()
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        on_update(source_index, repo_index, this, window, cx)
                    }))
            } else {
                Button::new("git-update-selected")
                    .icon(refresh_icon())
                    .label(if busy {
                        "Checking…"
                    } else {
                        "Check for updates"
                    })
                    .outline()
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| on_check(this, window, cx)))
            };

            div()
                .p_4()
                .rounded_xl()
                .border_1()
                .border_color(card_border)
                .bg(card_bg)
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(info_col)
                .child(div().flex_shrink_0().child(action_btn))
        };

        // Commits Section (Collapsible with git commit table)
        let commits_section = {
            let commits = if is_cloned {
                crate::repository_git::load_recent_commits(&target_path, 15)
            } else {
                Vec::new()
            };

            let header_el = div()
                .id("commits-toggle-header")
                .flex()
                .items_center()
                .justify_between()
                .p_3p5()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    on_toggle_commits(this, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(if commits_collapsed {
                            chevron_right_icon()
                                .w(px(14.))
                                .h(px(14.))
                                .text_color(cx.theme().muted_foreground)
                        } else {
                            chevron_down_icon()
                                .w(px(14.))
                                .h(px(14.))
                                .text_color(cx.theme().muted_foreground)
                        })
                        .child(
                            git_commit_icon()
                                .w(px(15.))
                                .h(px(15.))
                                .text_color(cx.theme().primary),
                        )
                        .child(div().font_bold().text_size(px(14.)).child("Commits"))
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(cx.theme().primary.opacity(0.1))
                                .text_color(cx.theme().primary)
                                .text_size(px(10.))
                                .font_bold()
                                .child(if is_cloned {
                                    format!("{} commits", commits.len())
                                } else {
                                    "Not cloned".to_string()
                                }),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(if commits_collapsed {
                            "Click to expand"
                        } else {
                            "Click to collapse"
                        }),
                );

            let mut container = div()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .overflow_hidden()
                .child(header_el);

            if !commits_collapsed {
                let content = if !is_cloned {
                    div()
                        .p_6()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Repository is not cloned locally yet. Clone this repository to view commit history.")
                } else if commits.is_empty() {
                    div()
                        .p_6()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child("No recent commits found.")
                } else {
                    let mut table = div()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .flex_col();

                    // Table Header
                    let table_header = div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_2()
                        .bg(cx.theme().muted.opacity(0.35))
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .text_size(px(10.))
                        .font_bold()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().w(px(80.)).child("COMMIT"))
                        .child(div().flex_1().min_w_0().child("MESSAGE"))
                        .child(div().w(px(140.)).child("AUTHOR"))
                        .child(div().w(px(110.)).text_right().child("DATE"));

                    table = table.child(table_header);

                    for commit in commits {
                        let row = div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .py_2p5()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.5))
                            .hover(|s| s.bg(cx.theme().muted.opacity(0.15)))
                            .child(
                                div().w(px(80.)).flex_shrink_0().child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .bg(cx.theme().secondary.opacity(0.15))
                                        .text_size(px(11.))
                                        .font_family(
                                            ".AppleSystemUIFontMonospaced, Menlo, monospace",
                                        )
                                        .text_color(cx.theme().primary)
                                        .child(commit.short_hash),
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .text_color(cx.theme().foreground)
                                    .truncate()
                                    .child(commit.message),
                            )
                            .child(
                                div()
                                    .w(px(140.))
                                    .flex_shrink_0()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(commit.author_name),
                            )
                            .child(
                                div()
                                    .w(px(110.))
                                    .flex_shrink_0()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .text_right()
                                    .truncate()
                                    .child(commit.date),
                            );
                        table = table.child(row);
                    }
                    table
                };

                container = container.child(content);
            }

            container
        };

        // Changelog / Scorecard Card
        let changelog_card = {
            let snapshot_releases = release_desk_store::workspace_releases::load_snapshot_releases(
                &project.directory,
                &repo.name,
            );
            let latest_snapshot_release = snapshot_releases.as_ref().and_then(|r| r.first());

            if let Some(rel) = latest_snapshot_release {
                let version_label = rel.tag_name.clone();
                let published_date = rel
                    .published_at
                    .as_deref()
                    .map(format_date_short)
                    .unwrap_or_else(|| "Recent".to_string());

                let changelog_items: Vec<String> = if let Some(desc) = &rel.description {
                    let items: Vec<String> = desc
                        .lines()
                        .map(|l| l.trim().trim_start_matches(['*', '-', ' ']).to_string())
                        .filter(|l| !l.is_empty())
                        .collect();
                    if items.is_empty() {
                        vec![desc.clone()]
                    } else {
                        items
                    }
                } else {
                    vec!["No release notes provided for this version.".to_string()]
                };

                let mut notes_list = div().flex().flex_col().gap_2();
                for item in changelog_items {
                    notes_list = notes_list.child(
                        div()
                            .flex()
                            .items_start()
                            .gap_2p5()
                            .child(
                                div()
                                    .w(px(5.))
                                    .h(px(5.))
                                    .mt(px(6.))
                                    .rounded_full()
                                    .bg(cx.theme().primary),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(item),
                            ),
                    );
                }

                let asset_count = rel.assets.len();
                let header_title = format!(
                    "{} {} · {} package{}",
                    repo.name,
                    version_label,
                    asset_count,
                    if asset_count == 1 { "" } else { "s" }
                );

                let mut assets_list = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border);

                for asset in &rel.assets {
                    let formatted_size = match asset.size {
                        Some(b) if b >= 1_000_000_000 => {
                            format!("{:.1} GB", b as f64 / 1_000_000_000.0)
                        }
                        Some(b) if b >= 1_000_000 => format!("{} MB", b / (1024 * 1024)),
                        Some(b) if b >= 1_000 => format!("{} KB", b / 1024),
                        Some(b) => format!("{b} B"),
                        None => "Unknown size".to_string(),
                    };
                    assets_list = assets_list.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(download_icon().w(px(14.)).h(px(14.)))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .child(format!("{} · {}", asset.name, formatted_size)),
                                    ),
                            )
                            .child(
                                div()
                                    .px_2p5()
                                    .py_1()
                                    .rounded_full()
                                    .bg(cx.theme().primary.opacity(0.1))
                                    .text_color(cx.theme().primary)
                                    .text_size(px(11.))
                                    .font_bold()
                                    .child("Ready in shelf"),
                            ),
                    );
                }

                div()
                    .p_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().font_bold().text_size(px(15.)).child(header_title))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(published_date),
                            ),
                    )
                    .child(notes_list)
                    .child(assets_list)
            } else {
                div()
                    .p_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        div()
                            .font_bold()
                            .text_size(px(14.))
                            .child("No releases published yet"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Release notes and packages will appear here when published upstream or recorded in the snapshot."),
                    )
            }
        };

        div()
            .flex_1()
            .min_w_0()
            .p_6()
            .flex()
            .flex_col()
            .gap_5()
            .h(px(640.))
            .overflow_y_scrollbar()
            .child(hero)
            .child(directory_row)
            .child(git_card)
            .child(commits_section)
            .child(changelog_card)
            .into_any_element()
    } else {
        div()
            .flex_1()
            .min_w_0()
            .h(px(640.))
            .p_8()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Select a repository to view details"),
            )
            .into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(700.))
        .gap_4()
        .child(header)
        .child(
            div()
                .flex()
                .flex_1()
                .min_h(px(640.))
                .border_1()
                .border_color(cx.theme().border)
                .rounded_xl()
                .overflow_hidden()
                .bg(cx.theme().background)
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

    #[test]
    fn external_editor_detection_and_fallback() {
        assert!(is_app_installed("Finder"));
        assert!(is_app_installed("Terminal"));
        assert!(!is_app_installed("DefinitelyNonExistentApp12345XYZ"));

        let editors = available_editors();
        assert!(!editors.is_empty());
        assert!(editors.iter().any(|e| e.id == "finder"));
        assert!(editors.iter().any(|e| e.id == "terminal"));
    }
}
