//! Repository status and guarded Git actions for a project workspace.
//!
//! Call these functions on a background thread: Git and network operations block.

use crate::workspace::WorkspaceProject;
use crate::workspace_sources::{RepoKey, SourceRepository, WorkspaceSource};
pub use release_desk_git::{CloneProgress, DestinationKind, GitCommit, SymlinkState};
use release_desk_git::{
    CloneState, CloneStatus, GitClient, GitCredentialContext, SyncOutcome, classify_destination,
};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// A concrete repository and destination resolved from a project.
#[derive(Clone, Debug)]
pub struct RepositoryTarget {
    pub key: RepoKey,
    pub source_index: usize,
    pub repo_index: usize,
    pub name: String,
    pub url: String,
    pub branch: String,
    pub path: PathBuf,
    /// Project folder used to recheck the path before each Git operation.
    pub project_root: PathBuf,
    /// Project-selected OS secret reference, never the secret itself.
    pub credential_ref: Option<String>,
    pub provider: String,
    pub account: String,
    pub endpoint: Option<String>,
}

/// Local state, or an actionable failure. A local inspection is not a remote check.
#[derive(Clone, Debug)]
pub struct RepositoryStatus {
    pub key: RepoKey,
    pub destination: DestinationKind,
    pub state: Result<CloneState, String>,
    pub fetched: bool,
}

/// Only a fresh remote check showing a clean, behind branch is eligible for a batch pull.
pub fn can_pull(status: &RepositoryStatus) -> bool {
    status.fetched
        && status
            .state
            .as_ref()
            .is_ok_and(|state| state.status == CloneStatus::Behind)
}

fn safe_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    !value.contains('\\')
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
}

/// Namespace returned by the provider for this repository.
pub fn repository_owner<'a>(
    source: &'a crate::workspace_sources::WorkspaceSource,
    repo: &'a crate::workspace_sources::SourceRepository,
) -> &'a str {
    repo.full_name
        .rsplit_once('/')
        .map_or(source.handle.as_str(), |(owner, _)| owner)
}

fn safe_namespace(value: &str) -> bool {
    !value.is_empty() && value.split('/').all(safe_component)
}

/// Resolve the on-disk location of a repository, respecting existing local repositories
/// whether they are at the root project directory, directly in the project directory,
/// or organized in a grouped subdirectory. Falls back to the project's disk layout.
pub fn resolve_target_path(
    project_root: &Path,
    disk_layout: release_desk_core::project::DiskLayout,
    owner: &str,
    repo_name: &str,
) -> PathBuf {
    if repo_name
        == project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
        && project_root.join(".git").exists()
    {
        return project_root.to_path_buf();
    }
    let direct = project_root.join(repo_name);
    if direct.join(".git").exists() {
        return direct;
    }
    let grouped = project_root.join(owner).join(repo_name);
    if grouped.join(".git").exists() {
        return grouped;
    }
    disk_layout.compute_clone_path(project_root, owner, repo_name)
}

/// Resolve selected repositories, deduplicating by [`RepoKey`] and rejecting unsafe paths.
pub fn targets(project: &WorkspaceProject) -> Result<Vec<RepositoryTarget>, String> {
    let mut result = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();
    let mut destinations = std::collections::HashSet::new();
    for (source_index, source) in project.sources.iter().enumerate() {
        for (repo_index, repo) in source
            .repositories
            .iter()
            .enumerate()
            .filter(|(_, r)| r.selected)
        {
            let key = project.repo_key(source, repo);
            if !seen_keys.insert(key.clone()) {
                return Err(format!(
                    "Repository {}/{} on {} is selected in more than one source.",
                    key.owner, key.name, key.host
                ));
            }
            let owner = repository_owner(source, repo);
            if !safe_namespace(owner) || !safe_component(&repo.name) {
                return Err(format!("Invalid repository path for {}", repo.name));
            }
            let path =
                resolve_target_path(&project.directory, project.disk_layout, owner, &repo.name);
            if !destinations.insert(path.clone()) {
                return Err(format!("More than one repository uses {}", path.display()));
            }
            validate_target_path(&project.directory, &path)?;
            result.push(RepositoryTarget {
                key,
                source_index,
                repo_index,
                name: repo.name.clone(),
                url: repo.clone_url.clone(),
                branch: repo.default_branch.clone(),
                path,
                project_root: project.directory.clone(),
                credential_ref: (source.provider == project.provider.as_str())
                    .then(|| project.credential_ref.clone())
                    .flatten(),
                provider: source.provider.clone(),
                account: project.account.clone(),
                endpoint: project.custom_host.clone(),
            });
        }
    }
    Ok(result)
}

/// Recheck each project-relative path component before using a previously resolved target.
fn validate_target_path(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Repository path is outside the project folder".to_string())?;
    let mut current = root.to_path_buf();
    for part in relative.components() {
        if !matches!(part, Component::Normal(_)) {
            return Err("Invalid repository path".into());
        }
        current.push(part.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "Repository path contains a symbolic link: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Cannot inspect repository path {}: {error}",
                    current.display()
                ));
            }
        }
    }
    Ok(())
}

/// Delete one repository checkout after verifying that it is a real directory
/// strictly beneath the project root. The project root itself is never removed.
pub fn delete_repository_checkout(project_root: &Path, path: &Path) -> Result<(), String> {
    validate_target_path(project_root, path)?;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Cannot inspect repository directory: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Repository checkout must be a real directory, not a symbolic link".into());
    }
    let project_root = project_root
        .canonicalize()
        .map_err(|error| format!("Cannot resolve project directory: {error}"))?;
    let checkout = path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve repository directory: {error}"))?;
    if checkout == project_root || !checkout.starts_with(&project_root) {
        return Err(
            "Repository checkout must be inside, and separate from, the project folder".into(),
        );
    }
    std::fs::remove_dir_all(&checkout)
        .map_err(|error| format!("Could not delete repository checkout: {error}"))
}

fn git_client(target: &RepositoryTarget) -> Result<GitClient, String> {
    let mut client = GitClient::new();
    if target.provider == "aws_codecommit" {
        let remote = url::Url::parse(&target.url)
            .map_err(|_| "CodeCommit requires a valid HTTPS clone URL".to_string())?;
        let host = remote
            .host_str()
            .ok_or("CodeCommit clone URL has no host")?;
        let expected_host = target
            .endpoint
            .as_deref()
            .map(|region| format!("git-codecommit.{region}.amazonaws.com"))
            .ok_or("CodeCommit project has no AWS region")?;
        if remote.scheme() != "https"
            || host != expected_host
            || !remote.username().is_empty()
            || remote.password().is_some()
        {
            return Err("CodeCommit clone URL is not a supported AWS HTTPS remote".into());
        }
        return Ok(client.with_codecommit_credentials());
    }
    if let Some(reference) = &target.credential_ref {
        let remote = url::Url::parse(&target.url)
            .map_err(|_| "Selected credential requires a valid HTTPS clone URL".to_string())?;
        if remote.scheme() != "https"
            || !remote.username().is_empty()
            || remote.password().is_some()
        {
            return Err(
                "Selected credential requires an HTTPS clone URL without embedded credentials"
                    .into(),
            );
        }
        let host = remote.host_str().ok_or("Clone URL has no host")?;
        let configured_origin = target
            .endpoint
            .as_deref()
            .and_then(|endpoint| url::Url::parse(endpoint).ok())
            .map(|endpoint| endpoint.origin());
        let valid_host = match target.provider.as_str() {
            "github" => host == "github.com",
            "gitlab" => host == "gitlab.com",
            "bitbucket" => host == "bitbucket.org",
            "gitea" => host == "gitea.com",
            "codeberg" => host == "codeberg.org",
            "azure_devops" => host == "dev.azure.com" || host.ends_with(".visualstudio.com"),
            "forgejo" | "custom" => configured_origin.as_ref() == Some(&remote.origin()),
            _ => false,
        };
        if !valid_host {
            return Err("Clone host does not match the selected provider".into());
        }
        if reference.starts_with("gcm:") {
            let provider = crate::workspace::ProviderChoice::from_source_provider(&target.provider);
            if provider.credential_manager_host() != Some(host)
                || provider
                    .credential_manager_reference(target.endpoint.as_deref())
                    .as_deref()
                    != Some(reference.as_str())
            {
                return Err("Git Credential Manager account belongs to another host".into());
            }
            return Ok(client.with_credential_manager());
        }
        let username = match target.provider.as_str() {
            "github" => "x-access-token",
            "gitlab" => "oauth2",
            "bitbucket" if reference.starts_with("oauth_") => "x-token-auth",
            "bitbucket" => "x-bitbucket-api-token-auth",
            "azure_devops" => "release-desk",
            _ => target.account.as_str(),
        };
        let executable = std::env::current_exe()
            .map_err(|error| format!("Cannot locate Git credential helper: {error}"))?;
        let authority = remote
            .port()
            .map_or_else(|| host.to_string(), |port| format!("{host}:{port}"));
        let credential = GitCredentialContext::for_provider(
            executable,
            reference.clone(),
            &target.url,
            &authority,
            username,
        )
        .map_err(|error| error.to_string())?;
        client = client.with_credential(credential);
    }
    Ok(client)
}

/// Classify a repository destination using the single source-of-truth classifier.
pub fn classify_repo_destination(
    project_directory: &Path,
    disk_layout: release_desk_core::project::DiskLayout,
    source: &WorkspaceSource,
    repo: &SourceRepository,
) -> DestinationKind {
    crate::modules::repo_cache::record_project_fs_access();
    let owner = repository_owner(source, repo);
    if !safe_namespace(owner) || !safe_component(&repo.name) {
        return DestinationKind::Unreadable("Invalid repository path".into());
    }
    let path = resolve_target_path(project_directory, disk_layout, owner, &repo.name);
    classify_destination(project_directory, &path)
}

/// True when a destination holds a leftover/incomplete directory eligible for repair or removal
/// (`EmptyDir`, `NonGitDir`, or empty `.git`-only repo; excludes `WrongRoot` and `Symlink`).
pub fn is_incomplete_clone(status: &RepositoryStatus) -> bool {
    matches!(
        status.destination,
        DestinationKind::EmptyDir | DestinationKind::NonGitDir
    ) || (status.destination == DestinationKind::GitRepo
        && status.state.as_ref().is_ok_and(|s| {
            s.status == CloneStatus::Error
                && s.local_revision.is_none()
                && s.remote_revision.is_none()
        }))
}

/// True only when an incomplete destination can be removed automatically without losing user files:
/// an empty directory, a directory containing only `.git` (no worktree files), or a partial-marked dir.
pub fn destination_allows_auto_remove(project_root: &Path, path: &Path) -> bool {
    if validate_target_path(project_root, path).is_err() {
        return false;
    }
    match classify_destination(project_root, path) {
        DestinationKind::EmptyDir => true,
        DestinationKind::GitRepo | DestinationKind::NonGitDir => {
            release_desk_git::is_dot_git_only_dir(path)
                || release_desk_git::has_partial_marker(path)
        }
        _ => release_desk_git::has_partial_marker(path),
    }
}

/// Remove ONLY safe automatic leftovers (`EmptyDir`, `.git`-only dir, or partial-marked dir).
/// Non-git directories with user content are never deleted automatically.
pub fn remove_incomplete_checkout(project_root: &Path, path: &Path) -> Result<(), String> {
    match classify_destination(project_root, path) {
        DestinationKind::Missing => Ok(()),
        DestinationKind::WrongRoot { .. } | DestinationKind::Symlink(_) => Err(
            "Cannot remove a directory inside another Git repository or a symbolic link.".into(),
        ),
        DestinationKind::Unreadable(error) => Err(error),
        DestinationKind::EmptyDir => {
            validate_target_path(project_root, path)?;
            std::fs::remove_dir(path)
                .map_err(|error| format!("Could not remove empty directory: {error}"))
        }
        DestinationKind::GitRepo | DestinationKind::NonGitDir
            if release_desk_git::is_dot_git_only_dir(path) =>
        {
            delete_repository_checkout(project_root, path)
        }
        _ if release_desk_git::has_partial_marker(path) => {
            delete_repository_checkout(project_root, path)
        }
        DestinationKind::NonGitDir | DestinationKind::GitRepo => Err(
            "Folder contains non-Git or worktree files and requires explicit confirmation to move to Trash."
                .into(),
        ),
    }
}

/// Move a verified project subdirectory to the macOS Trash (or `trash_dir` when specified for tests).
pub fn move_incomplete_dir_to_trash(project_root: &Path, path: &Path) -> Result<(), String> {
    move_dir_to_trash_in(project_root, path, None)
}

pub fn move_dir_to_trash_in(
    project_root: &Path,
    path: &Path,
    trash_dir: Option<&Path>,
) -> Result<(), String> {
    validate_target_path(project_root, path)?;
    match classify_destination(project_root, path) {
        DestinationKind::Missing => return Ok(()),
        DestinationKind::WrongRoot { .. } | DestinationKind::Symlink(_) => {
            return Err(
                "Refusing to Trash a symbolic link or directory inside another Git repository."
                    .into(),
            );
        }
        DestinationKind::Unreadable(error) => return Err(error),
        _ => {}
    }
    let root_canon = project_root
        .canonicalize()
        .map_err(|error| format!("Cannot resolve project directory: {error}"))?;
    let target_canon = path
        .canonicalize()
        .map_err(|error| format!("Cannot resolve repository directory: {error}"))?;
    if target_canon == root_canon || !target_canon.starts_with(&root_canon) {
        return Err("Target directory must stay strictly inside the project folder.".into());
    }

    if let Some(custom_trash) = trash_dir {
        std::fs::create_dir_all(custom_trash)
            .map_err(|error| format!("Cannot prepare Trash folder: {error}"))?;
        let base_name = target_canon
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "removed-folder".to_string());
        let mut dest = custom_trash.join(&base_name);
        let mut suffix = 1usize;
        while dest.exists() {
            dest = custom_trash.join(format!("{base_name}-{suffix}"));
            suffix += 1;
        }
        return std::fs::rename(&target_canon, &dest)
            .map_err(|error| format!("Could not move folder to Trash: {error}"));
    }

    #[cfg(target_os = "macos")]
    {
        if Path::new("/usr/bin/trash").exists() {
            let output = std::process::Command::new("/usr/bin/trash")
                .arg(&target_canon)
                .output()
                .map_err(|error| format!("Could not run macOS Trash utility: {error}"))?;
            if output.status.success() {
                return Ok(());
            }
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                "Could not move folder to macOS Trash.".into()
            } else {
                format!("Could not move folder to macOS Trash: {stderr}")
            });
        }
    }

    let home = std::env::var_os("HOME").ok_or("Cannot locate user Trash directory")?;
    let fallback_trash = PathBuf::from(home).join(".Trash");
    move_dir_to_trash_in(project_root, path, Some(&fallback_trash))
}

/// Clone one selected repository target atomically with progress and cancellation support.
pub fn clone_target_with_progress(
    target: &RepositoryTarget,
    allow_repair_empty_git: bool,
    cancelled: &AtomicBool,
    on_progress: impl FnMut(CloneProgress),
) -> Result<SyncOutcome, String> {
    validate_target_path(&target.project_root, &target.path)?;
    git_client(target)?
        .clone_atomic_cancellable(
            &target.project_root,
            &target.url,
            &target.path,
            &target.branch,
            allow_repair_empty_git,
            cancelled,
            on_progress,
        )
        .map_err(|error| error.to_string())
}

/// Clone one selected repository target.
pub fn clone_target(target: &RepositoryTarget) -> Result<SyncOutcome, String> {
    clone_target_with_progress(target, false, &AtomicBool::new(false), |_| {})
}

/// Clone one selected repository after revalidating the project and destination.
/// This never updates an existing non-empty directory. Run on a background thread.
pub fn clone_selected(
    project: &WorkspaceProject,
    source_index: usize,
    repo_index: usize,
) -> Result<SyncOutcome, String> {
    let target = targets(project)?
        .into_iter()
        .find(|target| target.source_index == source_index && target.repo_index == repo_index)
        .ok_or_else(|| "Repository is no longer selected".to_string())?;
    clone_target(&target)
}

/// Inspect local state without network access.
pub fn inspect(target: &RepositoryTarget) -> RepositoryStatus {
    crate::modules::repo_cache::record_project_fs_access();
    let destination = classify_destination(&target.project_root, &target.path);
    let state = match &destination {
        DestinationKind::Unreadable(error) => Err(error.clone()),
        DestinationKind::Symlink(_) => Err("Repository path is a symbolic link".into()),
        _ => validate_target_path(&target.project_root, &target.path).and_then(|()| {
            GitClient::new()
                .inspect(&target.path, &target.branch)
                .map_err(|e| e.to_string())
        }),
    };
    RepositoryStatus {
        key: target.key.clone(),
        destination,
        state,
        fetched: false,
    }
}

const AUTO_REFRESH_STALE_INDEX_COOLDOWN_SECS: u64 = 6 * 3600;

fn record_status_timing(
    client: &GitClient,
    repo_path: &Path,
    rec: &mut crate::modules::repo_cache::CachedRepoRecord,
    now: u64,
    elapsed_ms: f64,
    timed_out: bool,
    auto_refresh_stale_index: bool,
) {
    let index_bytes = std::fs::metadata(repo_path.join(".git/index")).map_or(0, |m| m.len());
    let expected_ms = (index_bytes as f64 / 4000.0).max(250.0) * 4.0;
    if timed_out {
        rec.status_slow_skipped = true;
        rec.status_slow_streak = rec.status_slow_streak.saturating_add(1);
        if rec.status_slow_streak >= 2 || index_bytes < 2 * 1024 * 1024 {
            rec.stale_index_suspected = true;
        }
    } else {
        rec.status_slow_skipped = false;
        if elapsed_ms > expected_ms.max(800.0) {
            rec.status_slow_streak = rec.status_slow_streak.saturating_add(1);
            if rec.status_slow_streak >= 2 {
                rec.stale_index_suspected = true;
            }
        } else {
            rec.status_slow_streak = 0;
            rec.stale_index_suspected = false;
        }
    }
    if auto_refresh_stale_index
        && rec.stale_index_suspected
        && now.saturating_sub(rec.last_index_refresh_at.unwrap_or(0))
            >= AUTO_REFRESH_STALE_INDEX_COOLDOWN_SECS
        && let Ok(true) = client.refresh_index_if_unlocked(repo_path)
    {
        rec.last_index_refresh_at = Some(now);
        rec.stale_index_suspected = false;
        rec.status_slow_streak = 0;
    }
    if timed_out || rec.status == CloneStatus::Error {
        rec.stale_locks = release_desk_git::detect_stale_locks(repo_path)
            .into_iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
    } else {
        rec.stale_locks.clear();
    }
}

fn run_dirty_check_with_stale_policy(
    client: &GitClient,
    repo_path: &Path,
    rec: &mut crate::modules::repo_cache::CachedRepoRecord,
    now: u64,
    auto_refresh_stale_index: bool,
) -> bool {
    let t0 = std::time::Instant::now();
    let mut timed_out = false;
    match client.inspect_tracked_dirty(repo_path) {
        Ok(dirty) => {
            let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
            rec.dirty_tracked = Some(dirty);
            rec.dirty_tracked_at = Some(now);
            record_status_timing(
                client,
                repo_path,
                rec,
                now,
                elapsed_ms,
                false,
                auto_refresh_stale_index,
            );
        }
        Err(release_desk_git::GitError::TimedOut { .. }) => {
            timed_out = true;
            record_status_timing(
                client,
                repo_path,
                rec,
                now,
                t0.elapsed().as_secs_f64() * 1000.0,
                true,
                auto_refresh_stale_index,
            );
        }
        Err(_) => {
            record_status_timing(
                client,
                repo_path,
                rec,
                now,
                t0.elapsed().as_secs_f64() * 1000.0,
                false,
                false,
            );
        }
    }
    timed_out
}

/// Incrementally revalidate one repository against its cached fingerprint (stat calls only when unchanged)
/// and run lock-free `-uno` tracked dirty check when requested. Returns `(record, timed_out)`.
pub fn revalidate_or_inspect_target(
    project_id: u128,
    target: &RepositoryTarget,
    cached: Option<&crate::modules::repo_cache::CachedRepoRecord>,
    check_dirty_tracked: bool,
    icon_map: &std::collections::HashMap<(String, String, String), std::path::PathBuf>,
    status_timeout: std::time::Duration,
) -> (crate::modules::repo_cache::CachedRepoRecord, bool) {
    revalidate_or_inspect_target_with_policy(
        project_id,
        target,
        cached,
        check_dirty_tracked,
        false,
        icon_map,
        status_timeout,
    )
}

fn detect_stale_lock_names(path: &Path) -> Vec<String> {
    release_desk_git::detect_stale_locks(path)
        .into_iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect()
}

/// Revalidate one repository with explicit opt-in `auto_refresh_stale_index` policy.
pub fn revalidate_or_inspect_target_with_policy(
    project_id: u128,
    target: &RepositoryTarget,
    cached: Option<&crate::modules::repo_cache::CachedRepoRecord>,
    check_dirty_tracked: bool,
    auto_refresh_stale_index: bool,
    icon_map: &std::collections::HashMap<(String, String, String), std::path::PathBuf>,
    status_timeout: std::time::Duration,
) -> (crate::modules::repo_cache::CachedRepoRecord, bool) {
    use crate::modules::repo_cache::{CachedRepoRecord, StoredFingerprint, now_unix_secs};
    crate::modules::repo_cache::record_project_fs_access();

    let now = now_unix_secs();
    let icon_key = (
        target.provider.to_ascii_lowercase(),
        target.key.owner.to_ascii_lowercase(),
        target.name.to_ascii_lowercase(),
    );
    let icon_path = icon_map
        .get(&icon_key)
        .cloned()
        .or_else(|| cached.and_then(|c| c.icon_path.clone()));

    // Pure-stat fast path: if we already classified this path as GitRepo, verify via
    // symlink_metadata + direct `.git` presence and ref mtime fingerprint before running any git process.
    let mut timed_out = false;
    let client = GitClient::new().with_timeout(status_timeout);
    if let Some(prev) = cached
        && prev.destination == DestinationKind::GitRepo
        && prev.fingerprint.is_some()
        && release_desk_git::path_looks_like_direct_git_repo(&target.project_root, &target.path)
    {
        let current_fp = release_desk_git::RepoFingerprint::capture(&target.path, &target.branch)
            .map(|fp| StoredFingerprint::from(&fp));
        if current_fp.is_some() && prev.fingerprint == current_fp {
            // Pure-stat hit: ZERO ref subprocesses! Refresh dirty_tracked only when requested.
            let mut updated = prev.clone();
            updated.icon_path = icon_path;
            updated.last_inspected_at = Some(now);
            updated.stale_locks = detect_stale_lock_names(&target.path);
            if check_dirty_tracked {
                timed_out = run_dirty_check_with_stale_policy(
                    &client,
                    &target.path,
                    &mut updated,
                    now,
                    auto_refresh_stale_index,
                );
            }
            return (updated, timed_out);
        }
    }

    let destination = classify_destination(&target.project_root, &target.path);
    if destination != DestinationKind::GitRepo {
        let status = match &destination {
            DestinationKind::Missing => CloneStatus::NotCloned,
            _ => CloneStatus::Error,
        };
        let err = match &destination {
            DestinationKind::Missing => None,
            DestinationKind::EmptyDir => Some("Empty directory at repository destination".into()),
            DestinationKind::NonGitDir => Some("Path is not a Git working tree".into()),
            DestinationKind::WrongRoot { .. } => {
                Some("Path is inside another Git working tree".into())
            }
            DestinationKind::Symlink(_) => Some("Repository path is a symbolic link".into()),
            DestinationKind::Unreadable(msg) => Some(msg.clone()),
            DestinationKind::GitRepo => unreachable!(),
        };
        return (
            CachedRepoRecord {
                project_id,
                key: target.key.clone(),
                destination,
                status,
                error_message: err,
                probed_remote_sha: cached.and_then(|c| c.probed_remote_sha.clone()),
                probe_failed_reason: cached.and_then(|c| c.probe_failed_reason.clone()),
                probe_issue: cached.and_then(|c| c.probe_issue.clone()),
                icon_path,
                latest_release_tag: cached.and_then(|c| c.latest_release_tag.clone()),
                latest_release_date: cached.and_then(|c| c.latest_release_date.clone()),
                latest_release_assets: cached
                    .map(|c| c.latest_release_assets.clone())
                    .unwrap_or_default(),
                installed_version: cached.and_then(|c| c.installed_version.clone()),
                last_inspected_at: Some(now),
                last_probed_at: cached.and_then(|c| c.last_probed_at),
                last_fetched_at: cached.and_then(|c| c.last_fetched_at),
                ..Default::default()
            },
            false,
        );
    }

    let current_fp = release_desk_git::RepoFingerprint::capture(&target.path, &target.branch)
        .map(|fp| StoredFingerprint::from(&fp));

    // Fingerprint miss or uncached: when `check_dirty_tracked` is true, read refs + ahead/behind + `-uno`
    // dirty status in ONE `status --porcelain=v2 --branch -uno` subprocess instead of 4-5.
    if check_dirty_tracked {
        let t0 = std::time::Instant::now();
        let rec = match client.inspect_refs_and_dirty_uno(&target.path, &target.branch) {
            Ok((state, ahead, behind, dirty)) => {
                let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
                let mut r = CachedRepoRecord {
                    project_id,
                    key: target.key.clone(),
                    destination: DestinationKind::GitRepo,
                    status: state.status,
                    branch: state.branch,
                    head_sha: state.local_revision,
                    remote_sha: state.remote_revision,
                    probed_remote_sha: cached.and_then(|c| c.probed_remote_sha.clone()),
                    ahead_count: ahead,
                    behind_count: behind,
                    error_message: state.message,
                    status_slow_skipped: false,
                    status_slow_streak: cached.map_or(0, |c| c.status_slow_streak),
                    stale_index_suspected: cached.is_some_and(|c| c.stale_index_suspected),
                    last_index_refresh_at: cached.and_then(|c| c.last_index_refresh_at),
                    stale_locks: detect_stale_lock_names(&target.path),
                    probe_failed_reason: cached.and_then(|c| c.probe_failed_reason.clone()),
                    probe_issue: cached.and_then(|c| c.probe_issue.clone()),
                    fetched: cached.is_some_and(|c| c.fetched),
                    dirty_tracked: Some(dirty),
                    dirty_tracked_at: Some(now),
                    has_untracked: None,
                    has_untracked_at: None,
                    recent_commits: cached.map(|c| c.recent_commits.clone()).unwrap_or_default(),
                    icon_path,
                    latest_release_tag: cached.and_then(|c| c.latest_release_tag.clone()),
                    latest_release_date: cached.and_then(|c| c.latest_release_date.clone()),
                    latest_release_assets: cached
                        .map(|c| c.latest_release_assets.clone())
                        .unwrap_or_default(),
                    installed_version: cached.and_then(|c| c.installed_version.clone()),
                    last_inspected_at: Some(now),
                    last_probed_at: cached.and_then(|c| c.last_probed_at),
                    last_fetched_at: cached.and_then(|c| c.last_fetched_at),
                    fingerprint: current_fp,
                };
                record_status_timing(
                    &client,
                    &target.path,
                    &mut r,
                    now,
                    elapsed_ms,
                    false,
                    auto_refresh_stale_index,
                );
                r
            }
            Err(err) => {
                if matches!(err, release_desk_git::GitError::TimedOut { .. }) {
                    timed_out = true;
                }
                let mut r = CachedRepoRecord {
                    project_id,
                    key: target.key.clone(),
                    destination: DestinationKind::GitRepo,
                    status: CloneStatus::Error,
                    error_message: Some(err.to_string()),
                    status_slow_skipped: timed_out,
                    status_slow_streak: cached.map_or(0, |c| c.status_slow_streak),
                    stale_index_suspected: cached.is_some_and(|c| c.stale_index_suspected),
                    last_index_refresh_at: cached.and_then(|c| c.last_index_refresh_at),
                    icon_path,
                    latest_release_tag: cached.and_then(|c| c.latest_release_tag.clone()),
                    latest_release_date: cached.and_then(|c| c.latest_release_date.clone()),
                    latest_release_assets: cached
                        .map(|c| c.latest_release_assets.clone())
                        .unwrap_or_default(),
                    installed_version: cached.and_then(|c| c.installed_version.clone()),
                    last_inspected_at: Some(now),
                    last_probed_at: cached.and_then(|c| c.last_probed_at),
                    last_fetched_at: cached.and_then(|c| c.last_fetched_at),
                    fingerprint: current_fp,
                    ..Default::default()
                };
                record_status_timing(
                    &client,
                    &target.path,
                    &mut r,
                    now,
                    t0.elapsed().as_secs_f64() * 1000.0,
                    timed_out,
                    auto_refresh_stale_index,
                );
                r
            }
        };
        return (rec, timed_out);
    }

    let rec = match client.inspect_refs_only(&target.path, &target.branch) {
        Ok((state, ahead, behind)) => CachedRepoRecord {
            project_id,
            key: target.key.clone(),
            destination: DestinationKind::GitRepo,
            status: state.status,
            branch: state.branch,
            head_sha: state.local_revision,
            remote_sha: state.remote_revision,
            probed_remote_sha: cached.and_then(|c| c.probed_remote_sha.clone()),
            ahead_count: ahead,
            behind_count: behind,
            error_message: state.message,
            status_slow_skipped: false,
            status_slow_streak: cached.map_or(0, |c| c.status_slow_streak),
            stale_index_suspected: cached.is_some_and(|c| c.stale_index_suspected),
            last_index_refresh_at: cached.and_then(|c| c.last_index_refresh_at),
            stale_locks: detect_stale_lock_names(&target.path),
            probe_failed_reason: cached.and_then(|c| c.probe_failed_reason.clone()),
            probe_issue: cached.and_then(|c| c.probe_issue.clone()),
            fetched: cached.is_some_and(|c| c.fetched),
            dirty_tracked: None,
            dirty_tracked_at: None,
            has_untracked: None,
            has_untracked_at: None,
            recent_commits: cached.map(|c| c.recent_commits.clone()).unwrap_or_default(),
            icon_path,
            latest_release_tag: cached.and_then(|c| c.latest_release_tag.clone()),
            latest_release_date: cached.and_then(|c| c.latest_release_date.clone()),
            latest_release_assets: cached
                .map(|c| c.latest_release_assets.clone())
                .unwrap_or_default(),
            installed_version: cached.and_then(|c| c.installed_version.clone()),
            last_inspected_at: Some(now),
            last_probed_at: cached.and_then(|c| c.last_probed_at),
            last_fetched_at: cached.and_then(|c| c.last_fetched_at),
            fingerprint: current_fp,
        },
        Err(err) => {
            if matches!(err, release_desk_git::GitError::TimedOut { .. }) {
                timed_out = true;
            }
            CachedRepoRecord {
                project_id,
                key: target.key.clone(),
                destination: DestinationKind::GitRepo,
                status: CloneStatus::Error,
                error_message: Some(err.to_string()),
                status_slow_skipped: timed_out,
                status_slow_streak: cached.map_or(0, |c| c.status_slow_streak),
                stale_index_suspected: cached.is_some_and(|c| c.stale_index_suspected),
                last_index_refresh_at: cached.and_then(|c| c.last_index_refresh_at),
                stale_locks: detect_stale_lock_names(&target.path),
                icon_path,
                latest_release_tag: cached.and_then(|c| c.latest_release_tag.clone()),
                latest_release_date: cached.and_then(|c| c.latest_release_date.clone()),
                latest_release_assets: cached
                    .map(|c| c.latest_release_assets.clone())
                    .unwrap_or_default(),
                installed_version: cached.and_then(|c| c.installed_version.clone()),
                last_inspected_at: Some(now),
                last_probed_at: cached.and_then(|c| c.last_probed_at),
                last_fetched_at: cached.and_then(|c| c.last_fetched_at),
                fingerprint: current_fp,
                ..Default::default()
            }
        }
    };
    (rec, timed_out)
}

/// Build a complete [`CachedRepoRecord`] from an already-computed [`RepositoryStatus`] so manual
/// checks, queued pulls, and refreshes update `statuses`, `presentations`, `cache_records`, and SQLite uniformly.
pub(crate) fn record_from_status(
    project_id: u128,
    target: &RepositoryTarget,
    status: &RepositoryStatus,
    cached: Option<&crate::modules::repo_cache::CachedRepoRecord>,
    icon_map: &std::collections::HashMap<(String, String, String), std::path::PathBuf>,
    load_commits: bool,
) -> crate::modules::repo_cache::CachedRepoRecord {
    use crate::modules::repo_cache::{
        CachedRepoRecord, StoredCommit, StoredFingerprint, now_unix_secs,
    };

    let now = now_unix_secs();
    let icon_key = (
        target.provider.to_ascii_lowercase(),
        target.key.owner.to_ascii_lowercase(),
        target.name.to_ascii_lowercase(),
    );
    let icon_path = icon_map
        .get(&icon_key)
        .cloned()
        .or_else(|| cached.and_then(|c| c.icon_path.clone()));
    let fingerprint = if status.destination == DestinationKind::GitRepo {
        release_desk_git::RepoFingerprint::capture(&target.path, &target.branch)
            .map(|fp| StoredFingerprint::from(&fp))
    } else {
        None
    };
    let client = GitClient::new().with_timeout(std::time::Duration::from_secs(10));

    match &status.state {
        Ok(cs) => {
            let (ahead_count, behind_count) = match (&cs.local_revision, &cs.remote_revision) {
                (Some(local), Some(remote)) if status.destination == DestinationKind::GitRepo => {
                    match cs.status {
                        CloneStatus::Current => (Some(0), Some(0)),
                        CloneStatus::NotCloned | CloneStatus::Error => (None, None),
                        _ if local == remote => (Some(0), Some(0)),
                        _ => match client.ahead_behind_counts(&target.path, local, remote) {
                            Ok((a, b)) => (Some(a), Some(b)),
                            Err(_) => (
                                cached.and_then(|c| c.ahead_count),
                                cached.and_then(|c| c.behind_count),
                            ),
                        },
                    }
                }
                _ => (None, None),
            };
            let recent_commits = if load_commits && status.destination == DestinationKind::GitRepo {
                load_recent_commits(&target.path, 15)
                    .into_iter()
                    .map(|c| StoredCommit {
                        hash: c.hash,
                        short_hash: c.short_hash,
                        author_name: c.author_name,
                        date: c.date,
                        message: c.message,
                    })
                    .collect()
            } else if status.destination == DestinationKind::GitRepo {
                cached
                    .filter(|c| c.head_sha == cs.local_revision)
                    .map(|c| c.recent_commits.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let (
                probed_remote_sha,
                probe_failed_reason,
                probe_issue,
                last_probed_at,
                last_fetched_at,
            ) = if status.fetched {
                (cs.remote_revision.clone(), None, None, Some(now), Some(now))
            } else {
                (
                    cached.and_then(|c| c.probed_remote_sha.clone()),
                    cached.and_then(|c| c.probe_failed_reason.clone()),
                    cached.and_then(|c| c.probe_issue.clone()),
                    cached.and_then(|c| c.last_probed_at),
                    cached.and_then(|c| c.last_fetched_at),
                )
            };
            CachedRepoRecord {
                project_id,
                key: target.key.clone(),
                destination: status.destination.clone(),
                status: cs.status,
                branch: cs.branch.clone(),
                head_sha: cs.local_revision.clone(),
                remote_sha: cs.remote_revision.clone(),
                probed_remote_sha,
                ahead_count,
                behind_count,
                error_message: cs.message.clone(),
                status_slow_skipped: false,
                status_slow_streak: cached.map_or(0, |c| c.status_slow_streak),
                stale_index_suspected: cached.is_some_and(|c| c.stale_index_suspected),
                last_index_refresh_at: cached.and_then(|c| c.last_index_refresh_at),
                stale_locks: detect_stale_lock_names(&target.path),
                probe_failed_reason,
                probe_issue,
                fetched: status.fetched || cached.is_some_and(|c| c.fetched),
                dirty_tracked: Some(cs.status == CloneStatus::Dirty),
                dirty_tracked_at: Some(now),
                has_untracked: cached.and_then(|c| c.has_untracked),
                has_untracked_at: cached.and_then(|c| c.has_untracked_at),
                recent_commits,
                icon_path,
                latest_release_tag: cached.and_then(|c| c.latest_release_tag.clone()),
                latest_release_date: cached.and_then(|c| c.latest_release_date.clone()),
                latest_release_assets: cached
                    .map(|c| c.latest_release_assets.clone())
                    .unwrap_or_default(),
                installed_version: cached.and_then(|c| c.installed_version.clone()),
                last_inspected_at: Some(now),
                last_probed_at,
                last_fetched_at,
                fingerprint,
            }
        }
        Err(err) => CachedRepoRecord {
            project_id,
            key: target.key.clone(),
            destination: status.destination.clone(),
            status: CloneStatus::Error,
            error_message: Some(err.clone()),
            stale_locks: detect_stale_lock_names(&target.path),
            probe_failed_reason: if status.fetched {
                Some(err.clone())
            } else {
                cached.and_then(|c| c.probe_failed_reason.clone())
            },
            fetched: status.fetched || cached.is_some_and(|c| c.fetched),
            icon_path,
            latest_release_tag: cached.and_then(|c| c.latest_release_tag.clone()),
            latest_release_date: cached.and_then(|c| c.latest_release_date.clone()),
            latest_release_assets: cached
                .map(|c| c.latest_release_assets.clone())
                .unwrap_or_default(),
            installed_version: cached.and_then(|c| c.installed_version.clone()),
            last_inspected_at: Some(now),
            last_probed_at: if status.fetched {
                Some(now)
            } else {
                cached.and_then(|c| c.last_probed_at)
            },
            last_fetched_at: if status.fetched {
                Some(now)
            } else {
                cached.and_then(|c| c.last_fetched_at)
            },
            fingerprint,
            ..Default::default()
        },
    }
}

/// Fetch remote refs and inspect; never changes the working tree.
pub fn check_for_updates(target: &RepositoryTarget) -> RepositoryStatus {
    crate::modules::repo_cache::record_project_fs_access();
    let destination = classify_destination(&target.project_root, &target.path);
    let state = match &destination {
        DestinationKind::Unreadable(error) => Err(error.clone()),
        DestinationKind::Symlink(_) => Err("Repository path is a symbolic link".into()),
        _ => validate_target_path(&target.project_root, &target.path).and_then(|()| {
            git_client(target)?
                .fetch_verified(&target.url, &target.path, &target.branch)
                .map_err(|e| e.to_string())
        }),
    };
    let fetched = state
        .as_ref()
        .is_ok_and(|s| !matches!(s.status, CloneStatus::NotCloned | CloneStatus::Error));
    RepositoryStatus {
        key: target.key.clone(),
        destination,
        state,
        fetched,
    }
}

/// Fetch and fast-forward default branch; clones if missing.
pub fn update(target: &RepositoryTarget) -> Result<SyncOutcome, String> {
    validate_target_path(&target.project_root, &target.path)?;
    let git = git_client(target)?;
    let state = git
        .inspect(&target.path, &target.branch)
        .map_err(|e| e.to_string())?;
    if state.status == CloneStatus::NotCloned {
        return git
            .clone_only(&target.url, &target.path, &target.branch)
            .map_err(|e| e.to_string());
    }
    git.sync_existing(&target.url, &target.path, &target.branch)
        .map_err(|e| e.to_string())
}

/// Pull an existing checkout without creating a missing repository.
pub fn pull_existing(target: &RepositoryTarget) -> Result<SyncOutcome, String> {
    validate_target_path(&target.project_root, &target.path)?;
    git_client(target)?
        .sync_existing(&target.url, &target.path, &target.branch)
        .map_err(|error| error.to_string())
}

/// Run a pull that can terminate an active Git fetch or fast-forward process.
pub fn pull_existing_cancellable(
    target: &RepositoryTarget,
    cancelled: &std::sync::atomic::AtomicBool,
    progress: impl FnMut(&'static str),
) -> Result<SyncOutcome, String> {
    validate_target_path(&target.project_root, &target.path)?;
    git_client(target)?
        .sync_existing_cancellable(
            &target.url,
            &target.path,
            &target.branch,
            cancelled,
            progress,
        )
        .map_err(|error| error.to_string())
}

/// Read recent commits from a local repository clone.
pub fn load_recent_commits(target_path: &Path, count: usize) -> Vec<GitCommit> {
    GitClient::new()
        .recent_commits(target_path, count)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        DestinationKind, RepoKey, RepositoryStatus, can_pull, delete_repository_checkout,
        resolve_target_path, safe_component, safe_namespace,
    };
    use release_desk_git::{CloneState, CloneStatus};

    #[test]
    fn batch_pull_requires_fresh_behind_status() {
        for (state, fetched, expected) in [
            (CloneStatus::Behind, true, true),
            (CloneStatus::Behind, false, false),
            (CloneStatus::Current, true, false),
            (CloneStatus::Dirty, true, false),
        ] {
            let status = RepositoryStatus {
                key: RepoKey::new("github", "github.com", "owner", "repo"),
                destination: DestinationKind::GitRepo,
                state: Ok(CloneState {
                    status: state,
                    branch: None,
                    local_revision: None,
                    remote_revision: None,
                    message: None,
                }),
                fetched,
            };
            assert_eq!(can_pull(&status), expected);
        }
    }

    #[test]
    fn rejects_path_traversal() {
        for invalid in ["", ".", "..", "../outside", "/tmp", "a/b"] {
            assert!(!safe_component(invalid));
        }
        assert!(safe_component("repo-name"));
        assert!(safe_namespace("group/subgroup"));
        assert!(!safe_namespace("group/../outside"));
        assert!(!safe_namespace("group/evil\\name"));
    }

    #[test]
    fn existing_repository_at_project_root_is_detected_as_the_checkout() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = temp.path().join("lightcraft");
        std::fs::create_dir_all(root.join(".git")).expect("repository metadata directory");

        let resolved = resolve_target_path(
            &root,
            release_desk_core::project::DiskLayout::Grouped,
            "storytold",
            "lightcraft",
        );

        assert_eq!(resolved, root);
    }

    #[test]
    fn checkout_deletion_is_limited_to_a_child_directory() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let root = temp.path().join("project");
        let checkout = root.join("repo");
        std::fs::create_dir_all(&checkout).expect("checkout directory");
        std::fs::write(checkout.join("README.md"), "repository").expect("checkout file");
        std::fs::write(root.join("project.txt"), "keep").expect("project file");

        delete_repository_checkout(&root, &checkout).expect("delete checkout");

        assert!(!checkout.exists());
        assert!(root.join("project.txt").exists());
        assert!(delete_repository_checkout(&root, &root).is_err());
    }

    #[test]
    fn incomplete_removal_rules_protect_user_files_wrong_root_and_symlinks() {
        use super::{
            destination_allows_auto_remove, move_dir_to_trash_in, remove_incomplete_checkout,
        };

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let trash = temp.path().join("trash");
        std::fs::create_dir_all(&root).unwrap();

        // 1. EmptyDir -> auto-remove allowed & succeeds.
        let empty = root.join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert!(destination_allows_auto_remove(&root, &empty));
        remove_incomplete_checkout(&root, &empty).unwrap();
        assert!(!empty.exists());

        // 2. `.git`-only dir (zero worktree files) -> auto-remove allowed & succeeds.
        let git_only = root.join("git-only");
        std::fs::create_dir_all(git_only.join(".git")).unwrap();
        std::fs::write(git_only.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert!(destination_allows_auto_remove(&root, &git_only));
        remove_incomplete_checkout(&root, &git_only).unwrap();
        assert!(!git_only.exists());

        // 3. `.git` + worktree user file -> NEVER auto-removed!
        let git_with_worktree = root.join("git-worktree");
        std::fs::create_dir_all(git_with_worktree.join(".git")).unwrap();
        std::fs::write(git_with_worktree.join("notes.txt"), "keep me").unwrap();
        assert!(!destination_allows_auto_remove(&root, &git_with_worktree));
        assert!(remove_incomplete_checkout(&root, &git_with_worktree).is_err());
        assert!(git_with_worktree.join("notes.txt").exists());

        // 4. NonGitDir with user content -> refused by auto-remove, moved cleanly to Trash on confirm.
        let non_git = root.join("non-git-notes");
        std::fs::create_dir_all(&non_git).unwrap();
        std::fs::write(non_git.join("draft.md"), "important user data").unwrap();
        assert!(!destination_allows_auto_remove(&root, &non_git));
        assert!(remove_incomplete_checkout(&root, &non_git).is_err());
        let (preview, count) = release_desk_git::list_directory_preview(&non_git, 5);
        assert_eq!(count, 1);
        assert_eq!(preview, vec!["draft.md".to_string()]);
        move_dir_to_trash_in(&root, &non_git, Some(&trash)).unwrap();
        assert!(!non_git.exists());
        assert_eq!(
            std::fs::read_to_string(trash.join("non-git-notes").join("draft.md")).unwrap(),
            "important user data"
        );

        // 5. Partial-marked folder -> auto-remove allowed & succeeds.
        let partial = root.join("marked-leftover");
        std::fs::create_dir_all(&partial).unwrap();
        std::fs::write(partial.join("partial-data"), "tmp").unwrap();
        std::fs::write(
            partial.join(".release-desk-partial"),
            release_desk_git::format_partial_marker(999_999, 0),
        )
        .unwrap();
        assert!(destination_allows_auto_remove(&root, &partial));
        remove_incomplete_checkout(&root, &partial).unwrap();
        assert!(!partial.exists());

        // 6. WrongRoot and Symlink -> refused for both auto-remove and Trash!
        let parent_repo = root.join("parent-repo");
        std::fs::create_dir_all(&parent_repo).unwrap();
        assert!(
            std::process::Command::new("git")
                .current_dir(&parent_repo)
                .args(["init", "-b", "main"])
                .output()
                .unwrap()
                .status
                .success()
        );
        let nested = parent_repo.join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("a.txt"), "x").unwrap();
        assert!(!destination_allows_auto_remove(&root, &nested));
        assert!(remove_incomplete_checkout(&root, &nested).is_err());
        assert!(move_dir_to_trash_in(&root, &nested, Some(&trash)).is_err());

        #[cfg(unix)]
        {
            let link = root.join("symlink-repo");
            std::os::unix::fs::symlink(temp.path(), &link).unwrap();
            assert!(!destination_allows_auto_remove(&root, &link));
            assert!(remove_incomplete_checkout(&root, &link).is_err());
            assert!(move_dir_to_trash_in(&root, &link, Some(&trash)).is_err());
            assert!(std::fs::symlink_metadata(&link).is_ok());
        }
    }

    #[test]
    fn duplicate_repo_across_two_sources_is_rejected_with_clear_error() {
        use crate::workspace::{ProviderChoice, WorkspaceProject};
        use crate::workspace_sources::{RepositoryListMode, SourceRepository, WorkspaceSource};
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let repo = |name: &str, url: &str| SourceRepository {
            name: name.into(),
            full_name: format!("Owner/{name}"),
            description: None,
            stars: 0,
            forks: 0,
            primary_language: None,
            is_private: false,
            is_fork: false,
            clone_url: url.into(),
            default_branch: "main".into(),
            selected: true,
        };
        let project = WorkspaceProject {
            id: 77,
            name: "Dedupe".into(),
            directory: dir,
            provider: ProviderChoice::GitHub,
            account: "Owner".into(),
            custom_host: None,
            credential_ref: None,
            disk_layout: release_desk_core::project::DiskLayout::Grouped,
            sources: vec![
                WorkspaceSource {
                    handle: "Owner".into(),
                    display_name: None,
                    avatar_url: None,
                    bio: None,
                    provider: "github".into(),
                    repositories: vec![repo("Tool", "https://github.com/Owner/Tool.git")],
                },
                WorkspaceSource {
                    handle: "owner".into(),
                    display_name: None,
                    avatar_url: None,
                    bio: None,
                    provider: "github".into(),
                    repositories: vec![repo("tool", "https://github.com/owner/tool.git")],
                },
            ],
            repository_list_mode: RepositoryListMode::Grouped,
        };
        let err = super::targets(&project).unwrap_err();
        assert!(
            err.contains("owner/tool") && err.contains("github.com"),
            "unexpected duplicate error: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn c4_stale_index_policy_respects_lock_opt_in_and_6h_cooldown() {
        use super::run_dirty_check_with_stale_policy;
        use release_desk_git::GitClient;
        use std::path::Path;
        use std::time::Duration;

        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let run_git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        };
        run_git(&["init", "-b", "main"]);
        run_git(&["config", "user.name", "Test"]);
        run_git(&["config", "user.email", "test@example.invalid"]);
        std::fs::write(repo.join("a.txt"), "hello\n").unwrap();
        run_git(&["add", "a.txt"]);
        run_git(&["commit", "-m", "init"]);

        // 1. User config before/after refresh must stay untouched; index.lock blocks refresh.
        let lock_path = repo.join(".git/index.lock");
        std::fs::write(&lock_path, "held").unwrap();
        let client = GitClient::new();
        assert!(!client.refresh_index_if_unlocked(&repo).unwrap());
        assert!(lock_path.exists());
        std::fs::remove_file(&lock_path).unwrap();
        assert!(client.refresh_index_if_unlocked(&repo).unwrap());

        // 2. Slow-status streak -> stale_index_suspected; opt-in=false never auto-refreshes;
        //    opt-in=true refreshes at most once per 6h (21_600s).
        let slow_client = GitClient::new().with_timeout(Duration::from_nanos(1));
        let mut rec = crate::modules::repo_cache::CachedRepoRecord {
            status: release_desk_git::CloneStatus::Current,
            ..Default::default()
        };
        assert!(run_dirty_check_with_stale_policy(
            &slow_client,
            &repo,
            &mut rec,
            1_000,
            false
        ));
        assert!(rec.stale_index_suspected);
        assert_eq!(rec.last_index_refresh_at, None);

        // With auto_refresh=true and a working client (or refresh trigger), refreshes once & sets cooldown.
        // Force a slow status timeout while keeping refresh itself functional? Use timeout=None client?
        // Simulate pre-existing stale_index_suspected=true with a tiny status timeout -> still uses
        // low-io default timeout on refresh_index_if_unlocked? refresh_index_if_unlocked inherits self.timeout!
        // So verify with timeout=5s + status_slow_streak: first call clears only if refresh runs cleanly.
        rec.status_slow_streak = 2;
        rec.stale_index_suspected = true;
        // Manually check cooldown gating around last_index_refresh_at:
        rec.last_index_refresh_at = Some(1_000);
        let working_client = GitClient::new();
        // Calling with auto_refresh=true inside 6h cooldown must NOT bump last_index_refresh_at:
        run_dirty_check_with_stale_policy(&working_client, &repo, &mut rec, 5_000, true);
        assert_eq!(rec.last_index_refresh_at, Some(1_000));

        // Simulate timeout again past 6h cooldown (now = 1_000 + 22_000): refresh runs via low-IO helper.
        // Use slow status script for `status` only? Directly test helper boundary with fake slow status script:
        let fake_git = temp.path().join("fake-git");
        let real_git = [
            "/opt/homebrew/bin/git",
            "/usr/local/bin/git",
            "/usr/bin/git",
        ]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or("git");
        std::fs::write(
            &fake_git,
            format!(
                "#!/bin/sh\nif [ \"$WARM\" = \"1\" ]; then exit 0; fi\nfor a in \"$@\"; do\n  if [ \"$a\" = \"status\" ]; then sleep 1; fi\ndone\nexec \"{real_git}\" \"$@\"\n"
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = std::fs::metadata(&fake_git).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake_git, perms).unwrap();
        }
        let _ = std::process::Command::new(&fake_git)
            .env("WARM", "1")
            .output();
        // Direct check: timeout on `status` keeps `refresh_index_if_unlocked` separate:
        let timed_client = GitClient::new()
            .with_executable(fake_git)
            .with_timeout(Duration::from_millis(80));
        rec.status_slow_streak = 1;
        rec.stale_index_suspected = false;
        // 1) Opt-in = false -> triggers Timeout -> marks stale, NO refresh:
        assert!(run_dirty_check_with_stale_policy(
            &timed_client,
            &repo,
            &mut rec,
            30_000,
            false
        ));
        assert!(rec.stale_index_suspected);
        assert_eq!(rec.last_index_refresh_at, Some(1_000));
        // 2) Opt-in = true past 6h -> runs refresh_index_if_unlocked & updates last_index_refresh_at:
        assert!(run_dirty_check_with_stale_policy(
            &timed_client,
            &repo,
            &mut rec,
            30_000,
            true
        ));
        assert!(!rec.stale_index_suspected);
        assert_eq!(rec.last_index_refresh_at, Some(30_000));
    }

    #[test]
    fn revalidate_and_record_from_status_populate_stale_locks_when_ok() {
        use super::{record_from_status, revalidate_or_inspect_target_with_policy};
        use std::time::Duration;

        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("owner").join("locked-repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git_cmd = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{:?} failed", args);
        };
        git_cmd(&["init", "-b", "main"]);
        git_cmd(&["config", "user.name", "Test"]);
        git_cmd(&["config", "user.email", "test@example.invalid"]);
        std::fs::write(repo.join("README.md"), "hello\n").unwrap();
        git_cmd(&["add", "README.md"]);
        git_cmd(&["commit", "-m", "init"]);

        let target = crate::repository_git::RepositoryTarget {
            provider: "github".into(),
            account: "owner".into(),
            source_index: 0,
            repo_index: 0,
            key: RepoKey::new("github", "github.com", "owner", "locked-repo"),
            name: "locked-repo".into(),
            url: "https://github.com/owner/locked-repo.git".into(),
            branch: "main".into(),
            credential_ref: None,
            endpoint: None,
            project_root: temp.path().to_path_buf(),
            path: repo.clone(),
        };

        std::fs::write(repo.join(".git").join("index.lock"), "").unwrap();
        let icons = std::collections::HashMap::new();

        // 1. Fingerprint miss (`check_dirty=true`) populates `stale_locks` even though status succeeds.
        let (miss_rec, _) = revalidate_or_inspect_target_with_policy(
            1,
            &target,
            None,
            true,
            false,
            &icons,
            Duration::from_secs(5),
        );
        assert_eq!(miss_rec.stale_locks, vec!["index.lock".to_string()]);

        // 2. Fingerprint hit also surfaces `stale_locks`.
        let (hit_rec, _) = revalidate_or_inspect_target_with_policy(
            1,
            &target,
            Some(&miss_rec),
            false,
            false,
            &icons,
            Duration::from_secs(5),
        );
        assert_eq!(hit_rec.stale_locks, vec!["index.lock".to_string()]);

        // 3. `record_from_status` populates `stale_locks` on both Ok and Err outcomes.
        let st_ok = crate::repository_git::inspect(&target);
        let from_ok = record_from_status(1, &target, &st_ok, None, &icons, false);
        assert_eq!(from_ok.stale_locks, vec!["index.lock".to_string()]);
    }
}
