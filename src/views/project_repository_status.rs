//! Repository master/detail browser matching the ArtCraft Release Desk design.

use crate::colors::rgb;
use crate::components::avatar::repo_icon_with_provider;
use crate::components::avatar::source_avatar;
use crate::components::badge::{branch_chip, language_chip, version_pill, visibility_chip};
use crate::components::icons::{
    UiIcon, android_icon, check_icon, chevron_down_icon, chevron_right_icon, code_icon,
    download_icon, external_link_icon, folder_icon, git_commit_icon, hammer_icon, refresh_icon,
    terminal_icon,
};
use crate::components::motion_button::Button;
use crate::controllers::git_controller::GitActivity;
use crate::controllers::job_registry::{ProjectSummaryCounts, RepoClonePhase, RepoStateEntry};
use crate::icon_catalog::{Language, LanguageColor};
use crate::modules::repo_cache::{CachedRepoRecord, now_unix_secs};
use crate::repository_git::{RepositoryStatus, repository_owner};
use crate::styles::CssExt as _;
use crate::workspace::WorkspaceProject;
use crate::workspace_sources::RepoKey;
use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, ElementId, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, deferred, div, px, uniform_list,
};
use release_desk_core::app_catalog::find_known_app;
use release_desk_git::CloneStatus;
use release_desk_store::source_fetcher::LanguageUsage;

/// The state of an on-demand repository language request.
#[derive(Clone, Debug, PartialEq)]
pub enum LanguageBreakdownState {
    Loading,
    Loaded(Vec<LanguageUsage>),
    Error(String),
}

fn row_activity(
    activity: Option<GitActivity>,
    pending: &std::collections::HashSet<RepoKey>,
    key: &RepoKey,
) -> Option<GitActivity> {
    activity.filter(|_| pending.contains(key))
}

fn language_color(name: &str) -> gpui_kit::Rgba {
    rgb(Language::from_name(name)
        .map_or(LanguageColor::Unknown, Language::color)
        .rgb())
}

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

#[derive(Clone, Debug, Default)]
pub struct RepositoryPresentation {
    pub icon_path: Option<std::path::PathBuf>,
    pub version_tag: Option<String>,
    pub latest_snapshot: Option<release_desk_store::workspace_releases::WorkspaceRelease>,
    pub present: bool,
    pub recent_commits: Vec<crate::repository_git::GitCommit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryAction {
    RemoveFromProject,
    DeleteFromDisk,
    RemoveAndDelete,
    RefreshIndex,
    RemoveStaleLocks,
}

/// Build one row's local presentation so it can be delivered with its status.
pub fn build_repository_presentation(
    project: &WorkspaceProject,
    source_index: usize,
    repo_index: usize,
) -> Option<RepositoryPresentation> {
    build_repository_presentation_with_icon(project, source_index, repo_index, None, true)
}

/// Build one row's presentation reusing an optional preloaded icon path and skipping eager `git log -15`
/// during batch inspections (commits stay lazy on selected repository only).
pub fn build_repository_presentation_with_icon(
    project: &WorkspaceProject,
    source_index: usize,
    repo_index: usize,
    preloaded_icon: Option<std::path::PathBuf>,
    include_commits: bool,
) -> Option<RepositoryPresentation> {
    let total_t = crate::modules::perf_trace::start();
    let source = project.sources.get(source_index)?;
    let repo = source.repositories.get(repo_index)?;
    let t = crate::modules::perf_trace::start();
    let repo_dir = crate::repository_git::resolve_target_path(
        &project.directory,
        project.disk_layout,
        repository_owner(source, repo),
        &repo.name,
    );
    let resolve_path_ns = crate::modules::perf_trace::elapsed_ns(t);
    let t = crate::modules::perf_trace::start();
    let icon_path = preloaded_icon.or_else(|| {
        crate::modules::icon_agent::resolve_icon_for(
            Some(&project.directory),
            source.provider.as_str(),
            repository_owner(source, repo),
            &repo.name,
        )
    });
    let icon_resolve_ns = crate::modules::perf_trace::elapsed_ns(t);
    let t = crate::modules::perf_trace::start();
    let latest_snapshot = release_desk_store::workspace_releases::load_snapshot_releases(
        &project.directory,
        &repo.name,
    )
    .and_then(|mut releases| {
        if releases.is_empty() {
            None
        } else {
            Some(releases.remove(0))
        }
    });
    let cached_release_ns = crate::modules::perf_trace::elapsed_ns(t);
    let version_tag = latest_snapshot
        .as_ref()
        .map(|release| release.tag_name.clone());
    let t = crate::modules::perf_trace::start();
    let present =
        release_desk_git::classify_destination(&project.directory, &repo_dir).is_git_repo();
    let present_check_ns = crate::modules::perf_trace::elapsed_ns(t);
    let t = crate::modules::perf_trace::start();
    let recent_commits = if present && include_commits {
        crate::repository_git::load_recent_commits(&repo_dir, 15)
    } else {
        Vec::new()
    };
    let recent_commits_ns = crate::modules::perf_trace::elapsed_ns(t);
    if total_t.is_some() {
        crate::modules::perf_trace::store_presentation(
            crate::modules::perf_trace::PresentationTimings {
                resolve_path_ns,
                icon_resolve_ns,
                cached_release_ns,
                present_check_ns,
                recent_commits_ns,
                total_ns: crate::modules::perf_trace::elapsed_ns(total_t),
            },
        );
    }
    Some(RepositoryPresentation {
        icon_path,
        version_tag,
        latest_snapshot,
        present,
        recent_commits,
    })
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
    static AVAILABLE_EDITORS: std::sync::OnceLock<Vec<ExternalEditor>> = std::sync::OnceLock::new();
    AVAILABLE_EDITORS.get_or_init(discover_editors).clone()
}

fn discover_editors() -> Vec<ExternalEditor> {
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

fn editor_icon(kind: EditorIconKind) -> UiIcon {
    match kind {
        EditorIconKind::VsCode | EditorIconKind::Cursor | EditorIconKind::Zed => code_icon(),
        EditorIconKind::Xcode => hammer_icon(),
        EditorIconKind::AndroidStudio => android_icon(),
        EditorIconKind::Terminal => terminal_icon(),
        EditorIconKind::Finder => folder_icon(),
    }
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
                    CloneStatus::Current if state.local_revision.is_none() && status.fetched => {
                        "No commits published"
                    }
                    CloneStatus::Current if state.local_revision.is_none() => "No local commits",
                    CloneStatus::Current if status.fetched => "Up to date",
                    CloneStatus::Current => "Check for updates",
                    CloneStatus::Behind if status.fetched => "Update available",
                    CloneStatus::Behind => "Update shown at last check",
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
                (label, detail, crate::repository_git::can_pull(status))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BadgeTone {
    Primary,
    Success,
    Warning,
    Danger,
    Muted,
}

fn format_age_minutes(ts: u64) -> String {
    let elapsed = now_unix_secs().saturating_sub(ts);
    if elapsed < 60 {
        "just now".to_string()
    } else if elapsed < 3600 {
        format!("{} min ago", elapsed / 60)
    } else if elapsed < 86_400 {
        format!("{} hr ago", elapsed / 3600)
    } else {
        format!("{} d ago", elapsed / 86_400)
    }
}

#[derive(Clone)]
struct VirtualRowData {
    key: RepoKey,
    name: String,
    owner_handle: String,
    description: String,
    provider: String,
    default_branch: String,
    icon_path: Option<std::path::PathBuf>,
    version_tag: Option<String>,
    has_new_release: bool,
    status_slow_skipped: bool,
    cached_age: Option<String>,
    badge_label: String,
    badge_tone: BadgeTone,
    is_active: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn render_project_repository_status<
    T: 'static,
    FCheck: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FToggleAutoFetch: Fn(u128, &mut T, &mut Context<T>) + Copy + 'static,
    FCheckSelected: Fn(RepoKey, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FUpdate: Fn(RepoKey, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FPullAll: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FClone: Fn(RepoKey, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FRetryClone: Fn(RepoKey, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FRemoveIncomplete: Fn(RepoKey, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FCloneAll: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FSelect: Fn(RepoKey, &mut T, &mut Context<T>) + Copy + 'static,
    FOpen: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FBrowseSources: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FToggleCommits: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FToggleLanguages: Fn(RepoKey, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
    FToggleEditorMenu: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FSelectEditor: Fn(String, &mut T, &mut Context<T>) + Copy + 'static,
    FOpenInEditor: Fn(std::path::PathBuf, String, &mut T, &mut Context<T>) + Copy + 'static,
    FRepositoryAction: Fn(RepoKey, RepositoryAction, &mut T, &mut Window, &mut Context<T>) + Copy + 'static,
>(
    project: &WorkspaceProject,
    repos: &std::collections::HashMap<RepoKey, RepoStateEntry>,
    clone_states: &std::collections::HashMap<RepoKey, RepoClonePhase>,
    selected: Option<&RepoKey>,
    search_input: &Entity<InputState>,
    inspect_busy: bool,
    clone_busy: bool,
    check_completed: usize,
    check_total: usize,
    activity: Option<GitActivity>,
    pending: &std::collections::HashSet<RepoKey>,
    visible_keys_sink: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<RepoKey>>>,
    list_scroll: &gpui_kit::UniformListScrollHandle,
    detail_scroll: &gpui_kit::ScrollHandle,
    last_checked_at: Option<u64>,
    auto_fetch_enabled: bool,
    commits_collapsed: bool,
    languages_expanded: &std::collections::HashSet<RepoKey>,
    language_breakdowns: &std::collections::HashMap<RepoKey, LanguageBreakdownState>,
    selected_editor_id: &str,
    editors: &[ExternalEditor],
    editor_menu_open: bool,
    cx: &Context<T>,
    on_check: FCheck,
    on_toggle_auto_fetch: FToggleAutoFetch,
    on_check_selected: FCheckSelected,
    on_update: FUpdate,
    on_pull_all: FPullAll,
    on_clone: FClone,
    on_retry_clone: FRetryClone,
    on_remove_incomplete: FRemoveIncomplete,
    on_clone_all: FCloneAll,
    on_select: FSelect,
    on_open: FOpen,
    on_browse_sources: FBrowseSources,
    on_toggle_commits: FToggleCommits,
    on_toggle_languages: FToggleLanguages,
    on_toggle_editor_menu: FToggleEditorMenu,
    on_select_editor: FSelectEditor,
    on_open_in_editor: FOpenInEditor,
    on_repository_action: FRepositoryAction,
) -> impl IntoElement {
    let busy = inspect_busy || clone_busy;
    let mut seen_keys = std::collections::HashSet::new();
    let all_repositories: Vec<_> = project
        .sources
        .iter()
        .flat_map(|source| {
            source
                .repositories
                .iter()
                .filter(|repo| repo.selected)
                .map(move |repo| (project.repo_key(source, repo), source, repo))
        })
        .filter(|(key, _, _)| seen_keys.insert(key.clone()))
        .collect();

    let query = search_input.read(cx).value().trim().to_lowercase();
    let repositories: Vec<_> = all_repositories
        .iter()
        .filter(|(_, _, repo)| {
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
        .filter(|key| repositories.iter().any(|(k, _, _)| k == *key))
        .cloned()
        .or_else(|| repositories.first().map(|(k, _, _)| k.clone()));

    let summary =
        ProjectSummaryCounts::from_repos(all_repositories.iter().map(|(key, _, _)| key), repos);
    let uncloned_count = summary.uncloned_count;

    let mut header_actions = div().flex().flex_wrap().items_center().gap_3();
    let pull_count = summary.pull_count;
    let clone_total = clone_states.len();
    let clone_done = clone_states.values().filter(|p| !p.is_active()).count();
    if clone_busy && clone_total > 0 {
        header_actions = header_actions.child(
            div()
                .px_3()
                .py_1p5()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().primary.opacity(0.35))
                .bg(cx.theme().primary.opacity(0.08))
                .text_size(px(12.))
                .font_bold()
                .text_color(cx.theme().primary)
                .child(format!("Cloning {clone_done}/{clone_total}…")),
        );
    }

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

    if pull_count > 0 {
        header_actions = header_actions.child(
            Button::new("git-pull-all-updates")
                .icon(download_icon())
                .label(format!("Pull available updates ({pull_count})"))
                .primary()
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| on_pull_all(this, window, cx))),
        );
    }

    let checked_count = summary.checked_count;
    let updates_count = summary.updates_count;
    let failed_count = summary.failed_count;

    let any_offline = all_repositories.iter().any(|(key, _, _)| {
        repos.get(key).is_some_and(|e| {
            e.record().probe_issue == Some(crate::modules::repo_cache::ProbeRepoIssue::Offline)
        })
    });

    let subtitle_text = if any_offline {
        format!(
            "Offline · showing cached state for {} repositories",
            all_repositories.len()
        )
    } else if last_checked_at.is_some() || checked_count > 0 || failed_count > 0 {
        let base = format!(
            "Checked {checked_count}/{}, {updates_count} updates, {failed_count} failed",
            all_repositories.len()
        );
        match last_checked_at {
            Some(ts) => format!("{base} · last checked {}", format_age_minutes(ts)),
            None => base,
        }
    } else {
        format!(
            "{} selected repositories across {} sources",
            all_repositories.len(),
            project.sources.len()
        )
    };

    let auto_fetch_project_id = project.id;
    header_actions = header_actions
        .child(
            Button::new("git-toggle-auto-fetch")
                .label(if auto_fetch_enabled {
                    "Auto-fetch: On"
                } else {
                    "Auto-fetch: Off"
                })
                .outline()
                .on_click(cx.listener(move |this, _, _, cx| {
                    on_toggle_auto_fetch(auto_fetch_project_id, this, cx)
                })),
        )
        .child(
            Button::new("git-check-updates")
                .icon(refresh_icon())
                .label(if busy && check_total > 0 {
                    let verb = match activity {
                        Some(GitActivity::Inspecting) => "Inspecting",
                        Some(GitActivity::Checking) => "Checking",
                        Some(GitActivity::Pulling) => "Pulling",
                        None => "Working",
                    };
                    format!("{verb} {check_completed}/{check_total}")
                } else if busy {
                    "Working…".to_string()
                } else {
                    "Check for updates".to_string()
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
                .child(div().css_body_muted(cx.theme()).child(subtitle_text)),
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

    let row_items: std::rc::Rc<Vec<VirtualRowData>> = std::rc::Rc::new(
        repositories
            .iter()
            .map(|(key, source, repo)| {
                let entry = repos.get(key);
                let status = entry.map(RepoStateEntry::status);
                let cache_rec = entry.map(RepoStateEntry::record);
                let clone_phase = clone_states.get(key);
                let active_row_activity = row_activity(activity, pending, key);
                let is_active = active.as_ref() == Some(key);
                let known = find_known_app(&repo.name);

                let is_incomplete = entry.is_some_and(RepoStateEntry::is_incomplete);
                let is_not_cloned =
                    !is_incomplete && entry.is_some_and(RepoStateEntry::is_missing_clone);
                let clone_status_val = cache_rec.map(CachedRepoRecord::effective_clone_status);

                let (badge_label, badge_tone) = if let Some(phase) =
                    clone_phase.filter(|p| !matches!(p, RepoClonePhase::Done))
                {
                    match phase {
                        RepoClonePhase::Queued => ("Queued…".to_string(), BadgeTone::Primary),
                        RepoClonePhase::Cloning { percent, .. } => (
                            match percent {
                                Some(p) => format!("Cloning {p}%"),
                                None => "Cloning…".to_string(),
                            },
                            BadgeTone::Primary,
                        ),
                        RepoClonePhase::Verifying => ("Verifying…".to_string(), BadgeTone::Primary),
                        RepoClonePhase::Failed(_) => {
                            ("Clone failed".to_string(), BadgeTone::Danger)
                        }
                        RepoClonePhase::Cancelled => ("Cancelled".to_string(), BadgeTone::Warning),
                        RepoClonePhase::Done => unreachable!(),
                    }
                } else if active_row_activity == Some(GitActivity::Pulling) {
                    ("Pulling…".to_string(), BadgeTone::Primary)
                } else if active_row_activity == Some(GitActivity::Checking) {
                    ("Checking…".to_string(), BadgeTone::Primary)
                } else if active_row_activity == Some(GitActivity::Inspecting) && status.is_none() {
                    ("Inspecting…".to_string(), BadgeTone::Muted)
                } else if cache_rec.is_some_and(|r| !r.stale_locks.is_empty()) {
                    ("Stale lock".to_string(), BadgeTone::Danger)
                } else if is_incomplete {
                    ("Incomplete clone".to_string(), BadgeTone::Warning)
                } else if is_not_cloned {
                    ("Not cloned".to_string(), BadgeTone::Muted)
                } else if cache_rec.and_then(|r| r.probe_issue.as_ref())
                    == Some(&crate::modules::repo_cache::ProbeRepoIssue::Offline)
                {
                    ("Offline".to_string(), BadgeTone::Muted)
                } else if let Some(issue) = cache_rec.and_then(|r| r.probe_issue.as_ref()) {
                    (issue.badge_label().to_string(), BadgeTone::Warning)
                } else if cache_rec
                    .and_then(|r| r.probe_failed_reason.as_ref())
                    .is_some()
                    || status.is_some_and(|s| s.state.is_err())
                {
                    ("Check failed - retry".to_string(), BadgeTone::Danger)
                } else if cache_rec.is_some_and(|r| r.stale_index_suspected) {
                    ("Status slow".to_string(), BadgeTone::Warning)
                } else if clone_status_val == Some(CloneStatus::Dirty) {
                    ("Dirty".to_string(), BadgeTone::Warning)
                } else if clone_status_val == Some(CloneStatus::Behind) {
                    let lbl = match cache_rec.and_then(|r| r.behind_count) {
                        Some(n) if n > 0 => format!("Behind {n}"),
                        _ => "Update available".to_string(),
                    };
                    (lbl, BadgeTone::Warning)
                } else if clone_status_val == Some(CloneStatus::Ahead) {
                    let lbl = match cache_rec.and_then(|r| r.ahead_count) {
                        Some(n) if n > 0 => format!("Ahead {n}"),
                        _ => "Ahead".to_string(),
                    };
                    (lbl, BadgeTone::Primary)
                } else if clone_status_val == Some(CloneStatus::Diverged) {
                    ("Diverged".to_string(), BadgeTone::Warning)
                } else if clone_status_val == Some(CloneStatus::Current) {
                    if status.is_some_and(|s| s.fetched) {
                        ("Up to date".to_string(), BadgeTone::Success)
                    } else if let Some(ts) =
                        cache_rec.and_then(|r| r.last_probed_at.or(r.last_inspected_at))
                    {
                        (
                            format!("Cached · {}", format_age_minutes(ts)),
                            BadgeTone::Muted,
                        )
                    } else {
                        ("Code current".to_string(), BadgeTone::Primary)
                    }
                } else if clone_status_val == Some(CloneStatus::Error) {
                    ("Error".to_string(), BadgeTone::Danger)
                } else {
                    let (label, _, _) = status_text(status);
                    (label.to_string(), BadgeTone::Muted)
                };

                let presentation = entry.map(RepoStateEntry::presentation);
                let cached_age = cache_rec
                    .and_then(|r| r.last_probed_at.or(r.last_inspected_at))
                    .map(format_age_minutes);

                VirtualRowData {
                    key: key.clone(),
                    name: repo.name.clone(),
                    owner_handle: source.handle.clone(),
                    description: repo
                        .description
                        .clone()
                        .filter(|d| !d.is_empty())
                        .or_else(|| known.as_ref().map(|k| k.description.clone()))
                        .unwrap_or_else(|| "Local Git workspace component".into()),
                    provider: source.provider.clone(),
                    default_branch: repo.default_branch.clone(),
                    icon_path: presentation.and_then(|item| item.icon_path.clone()),
                    version_tag: presentation.and_then(|item| item.version_tag.clone()),
                    has_new_release: cache_rec.is_some_and(CachedRepoRecord::has_new_release),
                    status_slow_skipped: cache_rec.is_some_and(|r| r.status_slow_skipped),
                    cached_age,
                    badge_label,
                    badge_tone,
                    is_active,
                }
            })
            .collect(),
    );

    let view = cx.entity();
    let row_items_for_list = row_items.clone();
    let visible_sink = visible_keys_sink.clone();
    let rows = uniform_list(
        "tracked-repos-virtual-list",
        row_items.len(),
        move |range, _window, cx| {
            let theme = cx.theme();
            if let Ok(mut vis) = visible_sink.lock() {
                vis.clear();
                for idx in range.clone() {
                    if let Some(row) = row_items_for_list.get(idx) {
                        vis.insert(row.key.clone());
                    }
                }
            }
            range
                .map(|row_idx| {
                    let item = &row_items_for_list[row_idx];
                    let (badge_bg, badge_fg) = match item.badge_tone {
                        BadgeTone::Primary => (theme.primary.opacity(0.12), theme.primary),
                        BadgeTone::Success => (theme.success.opacity(0.12), theme.success),
                        BadgeTone::Warning => (theme.warning.opacity(0.12), theme.warning),
                        BadgeTone::Danger => (theme.danger.opacity(0.12), theme.danger),
                        BadgeTone::Muted => (theme.secondary.opacity(0.15), theme.muted_foreground),
                    };

                    let mut tags = div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .flex_nowrap()
                        .overflow_hidden()
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(badge_bg)
                                .text_color(badge_fg)
                                .text_size(px(10.))
                                .font_bold()
                                .child(item.badge_label.clone()),
                        );
                    if item.has_new_release {
                        tags = tags.child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(theme.primary.opacity(0.14))
                                .text_color(theme.primary)
                                .text_size(px(10.))
                                .font_bold()
                                .child("New release"),
                        );
                    } else if item.status_slow_skipped {
                        tags = tags.child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(theme.warning.opacity(0.12))
                                .text_color(theme.warning)
                                .text_size(px(10.))
                                .font_bold()
                                .child("Status slow, skipped"),
                        );
                    } else if let Some(ref ver) = item.version_tag {
                        tags = tags.child(version_pill(ver, theme));
                    }
                    if let Some(ref age) = item.cached_age
                        && !item.badge_label.starts_with("Cached")
                        && !item.badge_label.starts_with("Checking")
                    {
                        tags = tags.child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme.muted_foreground)
                                .child(format!("· {age}")),
                        );
                    }
                    tags = tags.child(branch_chip(&item.default_branch, theme));

                    let select_key = item.key.clone();
                    let view = view.clone();
                    div()
                        .id(ElementId::Name(
                            format!("repo-row-{row_idx}-{}", item.name).into(),
                        ))
                        .when(item.is_active, |el| {
                            el.debug_selector(|| "repo-row-active".into())
                        })
                        .w_full()
                        .min_w_0()
                        .h(px(124.))
                        .overflow_hidden()
                        .flex()
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            let key = select_key.clone();
                            view.update(cx, |this, cx| on_select(key, this, cx));
                        })
                        .border_b_1()
                        .border_color(theme.border)
                        .bg(if item.is_active {
                            theme.primary.opacity(0.08)
                        } else {
                            theme.background
                        })
                        .child(div().w(px(3.)).flex_shrink_0().bg(if item.is_active {
                            theme.primary
                        } else {
                            gpui_kit::transparent_black()
                        }))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .p_3p5()
                                .pr_4()
                                .flex()
                                .flex_col()
                                .gap_1p5()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2p5()
                                        .child(repo_icon_with_provider(
                                            &item.name,
                                            34.,
                                            item.icon_path.as_deref(),
                                            Some(item.provider.as_str()),
                                        ))
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
                                                        .text_color(if item.is_active {
                                                            theme.primary
                                                        } else {
                                                            theme.foreground
                                                        })
                                                        .truncate()
                                                        .child(item.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(theme.muted_foreground)
                                                        .truncate()
                                                        .child(item.owner_handle.clone()),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme.muted_foreground)
                                        .truncate()
                                        .child(item.description.clone()),
                                )
                                .child(tags),
                        )
                })
                .collect()
        },
    )
    .track_scroll(list_scroll)
    .size_full();

    let rows_with_scrollbar = div()
        .relative()
        .flex_1()
        .w_full()
        .min_h_0()
        .overflow_hidden()
        .child(rows)
        .child(gpui_kit::component::scroll::Scrollbar::vertical(
            list_scroll,
        ));

    let rail = div()
        .w(px(330.))
        .flex_shrink_0()
        .border_r_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .flex()
        .flex_col()
        .min_h_0()
        .h_full()
        .overflow_hidden()
        .debug_selector(|| "repo-rail".into())
        .child(rail_header)
        .child(rows_with_scrollbar);

    // Detail Pane
    let active_entry = active.and_then(|key| {
        all_repositories
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(k, s, r)| (k.clone(), *s, *r))
    });

    let detail_panel = if let Some((repo_key, source, repo)) = active_entry {
        let entry = repos.get(&repo_key);
        let status = entry.map(RepoStateEntry::status);
        let cache_rec = entry.map(RepoStateEntry::record);
        let presentation = entry.map(RepoStateEntry::presentation);
        let clone_phase = clone_states.get(&repo_key);
        let active_row_activity = row_activity(activity, pending, &repo_key);
        let row_is_pulling = active_row_activity == Some(GitActivity::Pulling);

        let owner = crate::repository_git::repository_owner(source, repo);
        let target_path = crate::repository_git::resolve_target_path(
            &project.directory,
            project.disk_layout,
            owner,
            &repo.name,
        );

        let is_cloned = entry.is_some_and(RepoStateEntry::is_cloned);
        let is_incomplete = entry.is_some_and(RepoStateEntry::is_incomplete);
        let is_missing_clone = entry.is_some_and(RepoStateEntry::is_missing_clone);
        let is_inspecting =
            status.is_none() && active_row_activity == Some(GitActivity::Inspecting);
        let is_checking = status.is_none() && active_row_activity == Some(GitActivity::Checking);

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

        let detail_icon_path = presentation.and_then(|item| item.icon_path.as_deref());

        let mut meta_badges = div().flex().items_center().gap_2p5().pt_2().flex_wrap();
        if let Some(ref lang) = repo.primary_language {
            meta_badges = meta_badges.child(language_chip(lang, cx.theme()));
        }
        meta_badges = meta_badges.child(visibility_chip(repo.is_private, cx.theme()));
        meta_badges = meta_badges.child(branch_chip(&repo.default_branch, cx.theme()));
        if let Some(cache_rec) = cache_rec {
            if cache_rec.has_new_release()
                && let Some(ref tag) = cache_rec.latest_release_tag
            {
                meta_badges = meta_badges.child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(cx.theme().primary.opacity(0.14))
                        .text_color(cx.theme().primary)
                        .text_size(px(11.))
                        .font_bold()
                        .child(format!("New release {tag}")),
                );
            }
            if cache_rec.status_slow_skipped {
                meta_badges = meta_badges.child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(cx.theme().warning.opacity(0.12))
                        .text_color(cx.theme().warning)
                        .text_size(px(11.))
                        .font_bold()
                        .child("Status slow, skipped"),
                );
            }
            if let Some(ts) = cache_rec.last_probed_at.or(cache_rec.last_inspected_at) {
                meta_badges = meta_badges.child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(cx.theme().secondary.opacity(0.16))
                        .text_color(cx.theme().muted_foreground)
                        .text_size(px(11.))
                        .font_bold()
                        .child(format!("Cached · {}", format_age_minutes(ts))),
                );
            }
        }

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
                    .child(div().flex().flex_col().items_center().gap_1().child(
                        repo_icon_with_provider(
                            &repo.name,
                            60.,
                            detail_icon_path,
                            Some(source.provider.as_str()),
                        ),
                    ))
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
        for editor in editors {
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
        let git_card = if let Some(phase) =
            clone_phase.filter(|p| !matches!(p, RepoClonePhase::Done))
        {
            match phase {
                RepoClonePhase::Queued => div()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().primary.opacity(0.3))
                    .bg(cx.theme().primary.opacity(0.05))
                    .text_size(px(13.))
                    .text_color(cx.theme().primary)
                    .child("Queued for cloning…"),
                RepoClonePhase::Cloning { percent, detail } => div()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().primary.opacity(0.3))
                    .bg(cx.theme().primary.opacity(0.05))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_bold()
                            .text_size(px(13.))
                            .text_color(cx.theme().primary)
                            .child(match percent {
                                Some(p) => format!("Cloning repository ({p}%)…"),
                                None => "Cloning repository…".to_string(),
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(detail.clone()),
                    ),
                RepoClonePhase::Verifying => div()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().primary.opacity(0.3))
                    .bg(cx.theme().primary.opacity(0.05))
                    .text_size(px(13.))
                    .text_color(cx.theme().primary)
                    .child("Verifying cloned repository…"),
                RepoClonePhase::Failed(err) => {
                    let retry_key = repo_key.clone();
                    div()
                        .p_4()
                        .rounded_xl()
                        .border_1()
                        .border_color(cx.theme().danger.opacity(0.35))
                        .bg(cx.theme().danger.opacity(0.06))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .font_bold()
                                        .text_size(px(14.))
                                        .text_color(cx.theme().danger)
                                        .child("Clone failed"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().danger)
                                        .child(err.clone()),
                                ),
                        )
                        .child(
                            Button::new("git-retry-failed-clone")
                                .icon(refresh_icon())
                                .label("Retry")
                                .danger()
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_retry_clone(retry_key.clone(), this, window, cx)
                                })),
                        )
                }
                RepoClonePhase::Cancelled => {
                    let retry_key = repo_key.clone();
                    div()
                        .p_4()
                        .rounded_xl()
                        .border_1()
                        .border_color(cx.theme().warning.opacity(0.35))
                        .bg(cx.theme().warning.opacity(0.06))
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
                                        .child("Clone cancelled"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child("The clone was cancelled before completion."),
                                ),
                        )
                        .child(
                            Button::new("git-retry-cancelled-clone")
                                .icon(refresh_icon())
                                .label("Retry")
                                .outline()
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_retry_clone(retry_key.clone(), this, window, cx)
                                })),
                        )
                }
                RepoClonePhase::Done => unreachable!(),
            }
        } else if row_is_pulling {
            div()
                .p_4()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().primary.opacity(0.3))
                .bg(cx.theme().primary.opacity(0.05))
                .text_size(px(13.))
                .text_color(cx.theme().primary)
                .child("Pulling updates for this repository…")
        } else if is_inspecting {
            div()
                .p_4()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.08))
                .text_size(px(13.))
                .text_color(cx.theme().muted_foreground)
                .child("Inspecting this repository locally…")
        } else if is_checking {
            div()
                .p_4()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().primary.opacity(0.3))
                .bg(cx.theme().primary.opacity(0.05))
                .text_size(px(13.))
                .text_color(cx.theme().primary)
                .child("Checking this repository with its remote…")
        } else if is_incomplete {
            let retry_key = repo_key.clone();
            let remove_key = repo_key.clone();
            div()
                .p_4()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().warning.opacity(0.35))
                .bg(cx.theme().warning.opacity(0.06))
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_bold()
                                .text_size(px(14.))
                                .text_color(cx.theme().warning)
                                .child("Incomplete clone"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child("An empty or non-repository folder exists at this destination. Retry to replace an empty clone or Remove it."),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .flex_shrink_0()
                        .child(
                            Button::new("git-remove-incomplete")
                                .label("Remove")
                                .outline()
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_remove_incomplete(remove_key.clone(), this, window, cx)
                                })),
                        )
                        .child(
                            Button::new("git-retry-incomplete")
                                .icon(download_icon())
                                .label("Retry")
                                .primary()
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_retry_clone(retry_key.clone(), this, window, cx)
                                })),
                        ),
                )
        } else if is_missing_clone {
            let clone_key = repo_key.clone();
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
                        .label("Clone repository")
                        .primary()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_clone(clone_key.clone(), this, window, cx)
                        })),
                )
        } else {
            let (status_title, status_desc, is_behind, is_error): (String, String, bool, bool) = match status {
                None => (
                    if is_inspecting {
                        "Inspecting…".to_string()
                    } else if is_checking {
                        "Checking…".to_string()
                    } else {
                        "Not checked".to_string()
                    },
                    if is_inspecting {
                        "Inspecting this repository locally.".to_string()
                    } else if is_checking {
                        "Checking this repository against its remote.".to_string()
                    } else {
                        "Local repository status has not been inspected yet.".to_string()
                    },
                    false,
                    false,
                ),
                Some(st) => match &st.state {
                    Err(err) => (
                        "Update failed".to_string(),
                        format!("Error updating repository: {err}"),
                        false,
                        true,
                    ),
                    Ok(state) => match state.status {
                        CloneStatus::NotCloned => (
                            "Not cloned".to_string(),
                            "No local copy exists at the project destination.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Current if state.local_revision.is_none() => (
                            if st.fetched { "No commits published" } else { "No local commits" }.to_string(),
                            if st.fetched {
                                "The online repository has no commits yet. Check again after its first commit is published.".to_string()
                            } else {
                                state.message.clone().unwrap_or_else(|| "Check for updates to see whether the online repository has a first commit.".to_string())
                            },
                            false,
                            false,
                        ),
                        CloneStatus::Current if st.fetched => (
                            "Up to date".to_string(),
                            "Git working tree is in sync with latest tracked upstream commits.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Current => (
                            "Update check needed".to_string(),
                            "Your local copy is ready. Check for updates to compare it with the online repository.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Behind => {
                            let title = match cache_rec.and_then(|r| r.behind_count) {
                                Some(n) if n > 0 => format!("Behind {n}"),
                                _ => "Update available".to_string(),
                            };
                            (
                                title,
                                state.message.clone().unwrap_or_else(|| {
                                    "New upstream commits are available. Pull updates to bring your local repository up to date.".to_string()
                                }),
                                true,
                                false,
                            )
                        }
                        CloneStatus::Ahead => {
                            let title = match cache_rec.and_then(|r| r.ahead_count) {
                                Some(n) if n > 0 => format!("Ahead {n}"),
                                _ => "Local commits ahead".to_string(),
                            };
                            (
                                title,
                                "The local branch has commits not present in upstream remote.".to_string(),
                                false,
                                false,
                            )
                        }
                        CloneStatus::Diverged => (
                            "Branches diverged".to_string(),
                            "Local and remote branches have diverged; manual resolution required in Git.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Dirty => (
                            "Uncommitted changes".to_string(),
                            state.message.clone().unwrap_or_else(|| {
                                "Commit or stash local modifications before pulling updates."
                                    .to_string()
                            }),
                            false,
                            false,
                        ),
                        CloneStatus::Detached => (
                            "Detached HEAD".to_string(),
                            "Check out the default branch before pulling updates.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::OtherBranch => (
                            "Different branch".to_string(),
                            "Switch to the default branch before pulling updates.".to_string(),
                            false,
                            false,
                        ),
                        CloneStatus::Error => (
                            if cache_rec.is_some_and(|r| r.probe_failed_reason.is_some()) {
                                "Check failed - retry".to_string()
                            } else {
                                "Update error".to_string()
                            },
                            state.message.clone().unwrap_or_else(|| {
                                "Repository update failed or requires manual resolution in Git.".to_string()
                            }),
                            false,
                            true,
                        ),
                    },
                },
            };
            let stale_locks_present = cache_rec.is_some_and(|r| !r.stale_locks.is_empty());
            let lock_names_display = cache_rec
                .filter(|r| !r.stale_locks.is_empty())
                .map(|r| r.stale_locks.join(", "));
            let (status_title, status_desc, is_error) = if let Some(ref lock_names) =
                lock_names_display
            {
                (
                    "Git lock file blocking updates".to_string(),
                    format!(
                        "Found lock file(s) in .git ({lock_names}). Make sure no external Git command is running, then click Remove stale lock… before pulling or inspecting."
                    ),
                    true,
                )
            } else if let Some(ref fail) = cache_rec.and_then(|r| r.probe_failed_reason.as_ref()) {
                (
                    "Check failed - retry".to_string(),
                    format!("Remote check failed: {fail}"),
                    true,
                )
            } else if status.is_some_and(|s| s.state.is_err()) {
                ("Check failed - retry".to_string(), status_desc, true)
            } else if let Some(issue) = cache_rec.and_then(|r| r.probe_issue.as_ref()) {
                (
                    issue.badge_label().to_string(),
                    issue.detail_message(),
                    false,
                )
            } else {
                (status_title, status_desc, is_error)
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

            let can_repair = status.is_some_and(|status| {
                status.state.as_ref().is_ok_and(|state| {
                    state.local_revision.is_none() && state.remote_revision.is_some()
                })
            });
            let action_btn = if is_behind && !stale_locks_present {
                let update_key = repo_key.clone();
                Button::new("git-update-selected")
                    .icon(download_icon())
                    .label("Pull updates")
                    .primary()
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        on_update(update_key.clone(), this, window, cx)
                    }))
            } else if is_error {
                let err_key = repo_key.clone();
                Button::new("git-update-selected")
                    .icon(refresh_icon())
                    .label(if can_repair {
                        "Fix local copy"
                    } else {
                        "Check again"
                    })
                    .danger()
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if can_repair {
                            on_update(err_key.clone(), this, window, cx)
                        } else {
                            on_check_selected(err_key.clone(), this, window, cx)
                        }
                    }))
            } else {
                let check_key = repo_key.clone();
                Button::new("git-update-selected")
                    .icon(refresh_icon())
                    .label("Check for updates")
                    .outline()
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        on_check_selected(check_key.clone(), this, window, cx)
                    }))
            };

            let mut card_actions = div().flex().items_center().gap_2().flex_shrink_0();
            if cache_rec.is_some_and(|r| !r.stale_locks.is_empty()) {
                let lock_key = repo_key.clone();
                card_actions = card_actions.child(
                    Button::new("git-remove-stale-locks")
                        .label("Remove stale lock…")
                        .danger()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_repository_action(
                                lock_key.clone(),
                                RepositoryAction::RemoveStaleLocks,
                                this,
                                window,
                                cx,
                            )
                        })),
                );
            }
            if cache_rec.is_some_and(|r| r.stale_index_suspected) {
                let refresh_key = repo_key.clone();
                card_actions = card_actions.child(
                    Button::new("git-refresh-index")
                        .label("Refresh index")
                        .outline()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_repository_action(
                                refresh_key.clone(),
                                RepositoryAction::RefreshIndex,
                                this,
                                window,
                                cx,
                            )
                        })),
                );
            }
            card_actions = card_actions.child(action_btn);

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
                .child(card_actions)
        };

        let languages_section = {
            let key = repo_key.clone();
            let expanded = languages_expanded.contains(&key);
            let header = div()
                .id("languages-toggle-header")
                .flex()
                .items_center()
                .justify_between()
                .p_3p5()
                .cursor_pointer()
                .on_click(cx.listener({
                    let toggle_key = key.clone();
                    move |this, _, window, cx| {
                        on_toggle_languages(toggle_key.clone(), this, window, cx);
                    }
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            if expanded {
                                chevron_down_icon()
                            } else {
                                chevron_right_icon()
                            }
                            .w(px(14.))
                            .h(px(14.))
                            .text_color(cx.theme().muted_foreground),
                        )
                        .child(div().font_bold().text_size(px(14.)).child("Languages")),
                );
            let mut section = div()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .overflow_hidden()
                .child(header);

            if expanded {
                let mut content = div()
                    .p_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .flex()
                    .flex_col()
                    .gap_3();
                match language_breakdowns.get(&key) {
                    Some(LanguageBreakdownState::Loaded(languages)) if !languages.is_empty() => {
                        let mut bar = div()
                            .w_full()
                            .max_w(px(540.))
                            .h(px(9.))
                            .rounded_full()
                            .overflow_hidden()
                            .flex();
                        let mut legend = div().flex().flex_wrap().items_center().gap_3();
                        for language in languages {
                            let color = language_color(&language.name);
                            bar = bar.child(
                                div()
                                    .h_full()
                                    .flex_basis(px(0.))
                                    .flex_grow(language.percentage as f32)
                                    .bg(color),
                            );
                            legend = legend.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .text_size(px(12.))
                                    .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(color))
                                    .child(div().font_bold().child(language.name.clone()))
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("{:.1}%", language.percentage)),
                                    ),
                            );
                        }
                        content = content.child(bar).child(legend);
                    }
                    Some(LanguageBreakdownState::Loaded(_)) => {
                        content =
                            content.child("No language statistics reported for this repository.");
                    }
                    Some(LanguageBreakdownState::Error(error)) => {
                        content = content.child(format!("{error} · Collapse and reopen to retry."));
                    }
                    Some(LanguageBreakdownState::Loading) | None => {
                        content = content.child("Loading language statistics…");
                    }
                }
                section = section.child(content);
            }
            section
        };

        // Commits Section (Collapsible with git commit table)
        let commits_section = {
            let commits = presentation
                .map(|item| item.recent_commits.clone())
                .unwrap_or_default();

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
                                } else if is_inspecting {
                                    "Inspecting…".to_string()
                                } else if is_checking {
                                    "Checking…".to_string()
                                } else if status.is_none() {
                                    "Not checked".to_string()
                                } else if is_incomplete {
                                    "Incomplete clone".to_string()
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
                        .child(if is_inspecting {
                            "Inspecting local commit history…"
                        } else if is_checking {
                            "Checking remote state; local repository details are loading…"
                        } else if status.is_none() {
                            "Local repository status has not been inspected yet."
                        } else if is_incomplete {
                            "Repository clone is incomplete. Retry or remove the incomplete folder."
                        } else {
                            "Repository is not cloned locally yet. Clone this repository to view commit history."
                        })
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

        let action_row = |id: &'static str,
                          title: &'static str,
                          description: &'static str,
                          label: &'static str,
                          action: RepositoryAction,
                          destructive: bool| {
            let button = Button::new(id).label(label);
            let button = if destructive {
                button.danger()
            } else {
                button.outline()
            };
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .px_4()
                .py_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_medium().text_size(px(13.)).child(title))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        ),
                )
                .child(button.on_click(cx.listener({
                    let action_key = repo_key.clone();
                    move |this, _, window, cx| {
                        on_repository_action(action_key.clone(), action, this, window, cx)
                    }
                })))
        };
        let danger_zone = div()
            .rounded_xl()
            .border_1()
            .border_color(cx.theme().danger.opacity(0.35))
            .overflow_hidden()
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(cx.theme().danger.opacity(0.25))
                    .child(div().font_bold().text_size(px(17.)).child("Danger zone"))
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Changes below affect project tracking or the local checkout."),
                    ),
            )
            .child(action_row(
                "repo-remove-from-project",
                "Remove from project",
                "Stop tracking this repository. Files on disk stay untouched.",
                "Remove from project",
                RepositoryAction::RemoveFromProject,
                false,
            ))
            .child(
                div()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(action_row(
                        "repo-delete-from-disk",
                        "Delete local checkout",
                        "Delete files from disk and keep the repository in this project.",
                        "Delete from disk…",
                        RepositoryAction::DeleteFromDisk,
                        true,
                    )),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(action_row(
                        "repo-remove-and-delete",
                        "Remove and delete",
                        "Remove it from this project and delete its local checkout.",
                        "Remove and delete…",
                        RepositoryAction::RemoveAndDelete,
                        true,
                    )),
            );

        let detail_id = ElementId::Name(
            format!(
                "repo-detail-{}-{}-{}-{}",
                repo_key.provider, repo_key.host, repo_key.owner, repo_key.name
            )
            .into(),
        );
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            .overflow_hidden()
            .debug_selector(|| "repo-detail-panel".into())
            .child(
                div()
                    .id(detail_id)
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(detail_scroll)
                    .child(
                        div()
                            .w_full()
                            .p_6()
                            .pb_10()
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(hero)
                            .child(languages_section)
                            .child(directory_row)
                            .child(git_card)
                            .child(commits_section)
                            .child(danger_zone),
                    ),
            )
            .vertical_scrollbar(detail_scroll)
            .into_any_element()
    } else {
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .min_h_0()
            .debug_selector(|| "repo-detail-panel".into())
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
        .min_h_0()
        .h_full()
        .gap_4()
        .child(header)
        .child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .h_full()
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
    use release_desk_git::{CloneState, CloneStatus, DestinationKind};

    #[test]
    fn pull_activity_is_scoped_to_pending_repositories() {
        let key_a = RepoKey::new("github", "github.com", "owner", "alpha");
        let key_b = RepoKey::new("github", "github.com", "owner", "beta");
        let pending = std::collections::HashSet::from([key_a.clone()]);
        assert_eq!(
            row_activity(Some(GitActivity::Pulling), &pending, &key_a),
            Some(GitActivity::Pulling)
        );
        assert_eq!(
            row_activity(Some(GitActivity::Pulling), &pending, &key_b),
            None
        );
        assert_eq!(row_activity(None, &pending, &key_a), None);
    }

    fn status(state: CloneStatus, fetched: bool) -> RepositoryStatus {
        RepositoryStatus {
            key: RepoKey::new("github", "github.com", "owner", "repo"),
            destination: if state == CloneStatus::NotCloned {
                DestinationKind::Missing
            } else {
                DestinationKind::GitRepo
            },
            state: Ok(CloneState {
                status: state,
                branch: None,
                local_revision: Some("local".into()),
                remote_revision: Some("remote".into()),
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
            "Check for updates"
        );
        assert_eq!(
            status_text(Some(&status(CloneStatus::Current, true))).0,
            "Up to date"
        );
        assert_eq!(
            status_text(Some(&status(CloneStatus::Behind, false))).0,
            "Update shown at last check"
        );
        let mut empty = status(CloneStatus::Current, true);
        empty.state.as_mut().unwrap().local_revision = None;
        assert_eq!(status_text(Some(&empty)).0, "No commits published");
        empty.fetched = false;
        assert_eq!(status_text(Some(&empty)).0, "No local commits");
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
