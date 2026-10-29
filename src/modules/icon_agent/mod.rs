//! AI-powered repository icon discovery and persistent caching.
//!
//! Architecture:
//! 1. **Scanner** (Tool 1) — Finds candidate image files in a repository
//! 2. **Selection** (Tool 2) — Applies shared asset policy, then Kev or local ranking
//! 3. **Cache** (Tool 3) — Copies the chosen icon to `.release-desk/icons/` and records in SQLite
//!
//! The actual image files are saved in the `.release-desk/icons/` folder.
//! Only metadata (file path, resolution, scan time) is recorded in SQLite.

pub mod cache;
pub mod db;
pub mod kev;
pub(crate) mod policy;
pub mod scanner;

pub use cache::{cache_icon, cache_icon_in, icons_dir, release_desk_dir};
pub use db::{IconRecord, open_icon_db, open_icon_db_for, query_icon, query_icon_for, record_icon};
pub use kev::{KevClient, KevDecisionResult, ScoredCandidate};
pub use scanner::{ImageCandidate, scan_repo};

use std::path::{Path, PathBuf};

static ICON_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug)]
pub struct IconRefreshProgress {
    pub completed: usize,
    pub total: usize,
    pub repository: String,
}

#[derive(Clone, Debug)]
pub struct IconRefreshReport {
    pub total: usize,
    pub updated: usize,
    pub failures: Vec<String>,
}

/// Resolve an icon for a repository from the persistent cache.
/// This is the fast path called from UI views — no scanning, no AI.
pub fn resolve_icon(provider: &str, owner: &str, repo_name: &str) -> Option<PathBuf> {
    resolve_icon_for(None, provider, owner, repo_name)
}

/// Resolve an icon for a repository from the project or global persistent cache.
pub fn resolve_icon_for(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
) -> Option<PathBuf> {
    query_icon_for(project_dir, provider, owner, repo_name)
        .map(|record| PathBuf::from(record.cached_path))
        .filter(|path| path.is_file())
        .or_else(|| cache::find_cached(project_dir, provider, owner, repo_name))
}

/// Full pipeline: scan → select → cache.
pub fn scan_and_cache_icon(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
    repo_dir: &Path,
    kev: &KevClient,
) -> Option<PathBuf> {
    try_scan_and_cache_icon(
        project_dir,
        provider,
        owner,
        repo_name,
        repo_dir,
        kev,
        false,
    )
    .ok()
}

#[allow(clippy::too_many_arguments)]
fn try_scan_and_cache_icon(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
    repo_dir: &Path,
    kev: &KevClient,
    force: bool,
) -> Result<PathBuf, String> {
    // Check if already cached
    let previous = resolve_icon_for(project_dir, provider, owner, repo_name);
    if !force {
        if let Some(existing) = previous {
            return Ok(existing);
        }
    }

    // Tool 1: Scan for candidates
    let candidates = scan_repo(repo_dir, repo_name);
    if candidates.is_empty() {
        return Err("no icon candidates found".into());
    }

    // Tool 2: Apply the shared policy and rank the complete candidate set.
    let decision = kev
        .rank_and_filter_top10(&candidates, repo_name)
        .ok_or("no valid icon candidate selected")?;
    let selected = decision.top_choice;

    // Tool 3: Cache image to disk and record path in DB
    let source_path = repo_dir.join(&selected.relative_path);
    if !source_path.is_file() {
        return Err(format!(
            "selected file is missing: {}",
            selected.relative_path
        ));
    }

    let cached_path = cache_icon_in(&source_path, project_dir, provider, owner, repo_name)
        .ok_or("could not write the selected icon cache file")?;

    let resolution = crate::modules::artwork::read_icon_resolution(&cached_path);
    let _write_guard = ICON_WRITE_LOCK
        .lock()
        .map_err(|_| "icon database lock is unavailable")?;
    db::record_ai_confirmed_icon(
        project_dir,
        provider,
        owner,
        repo_name,
        &selected.relative_path,
        &cached_path,
        resolution,
        &decision.backend,
    );

    if resolve_icon_for(project_dir, provider, owner, repo_name).as_deref()
        != Some(cached_path.as_path())
    {
        return Err("could not save the icon database record".into());
    }
    if let (Some(project_dir), Some(previous)) = (project_dir, previous)
        && previous != cached_path
        && previous.starts_with(project_dir.join(".release-desk/icons"))
    {
        let _ = std::fs::remove_file(previous);
    }
    Ok(cached_path)
}

/// Select an icon for a newly cloned repository.
pub fn generate_for_target(
    project: &crate::workspace::WorkspaceProject,
    target: &crate::repository_git::RepositoryTarget,
) -> Option<PathBuf> {
    let source = project.sources.get(target.source_index)?;
    let repo = source.repositories.get(target.repo_index)?;
    let owner = crate::repository_git::repository_owner(source, repo);
    scan_and_cache_icon(
        Some(&project.directory),
        &source.provider,
        owner,
        &repo.name,
        &target.path,
        &KevClient::default(),
    )
}

/// Remove only Release Desk's project-local cache, including selected icons.
pub fn clear_project_cache(project_dir: &Path) -> std::io::Result<()> {
    let _write_guard = ICON_WRITE_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("icon database lock is unavailable"))?;
    if !project_dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "project folder is missing",
        ));
    }
    let cache = project_dir.join(".release-desk");
    match std::fs::symlink_metadata(&cache) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "project cache is a symlink",
        )),
        Ok(metadata) if metadata.is_dir() => {
            let icons = cache.join("icons");
            match std::fs::symlink_metadata(&icons) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    std::fs::remove_dir_all(icons)?
                }
                Ok(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "icon cache is not a directory",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            for name in ["agent.db", "agent.db-wal", "agent.db-shm"] {
                match std::fs::remove_file(cache.join(name)) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "project cache is not a directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Rerun the agent for every known cloned repository, keeping old icons on failure.
pub fn recreate_project_icons(
    project: &crate::workspace::WorkspaceProject,
) -> Result<IconRefreshReport, String> {
    recreate_project_icons_with_progress(project, |_| {})
}

pub fn recreate_project_icons_with_progress(
    project: &crate::workspace::WorkspaceProject,
    progress: impl Fn(IconRefreshProgress) + Sync,
) -> Result<IconRefreshReport, String> {
    let root = project
        .directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut cloned = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for source in &project.sources {
        for repo in &source.repositories {
            let owner = crate::repository_git::repository_owner(source, repo);
            let path = crate::repository_git::resolve_target_path(
                &project.directory,
                project.disk_layout,
                owner,
                &repo.name,
            );
            if !path.join(".git").exists() {
                continue;
            }
            let path = path.canonicalize().map_err(|error| error.to_string())?;
            if !path.starts_with(&root) {
                return Err(format!("repository path leaves the project: {}", repo.name));
            }
            if seen.insert((
                source.provider.clone(),
                owner.to_string(),
                repo.name.clone(),
            )) {
                cloned.push((
                    source.provider.clone(),
                    owner.to_string(),
                    repo.name.clone(),
                    path,
                ));
            }
        }
    }
    if cloned.is_empty() {
        return Err("No cloned repositories were found in this project.".into());
    }
    let total = cloned.len();
    progress(IconRefreshProgress {
        completed: 0,
        total,
        repository: String::new(),
    });
    let completed = std::sync::atomic::AtomicUsize::new(0);
    let results = crate::modules::parallel::map_bounded(
        &cloned,
        crate::modules::parallel::IO_WORKERS,
        |(provider, owner, name, path)| {
            let result = try_scan_and_cache_icon(
                Some(&project.directory),
                provider,
                owner,
                name,
                path,
                &KevClient::default(),
                true,
            );
            progress(IconRefreshProgress {
                completed: completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
                total,
                repository: name.clone(),
            });
            result.map_err(|error| format!("{owner}/{name}: {error}"))
        },
    );
    let updated = results.iter().filter(|result| result.is_ok()).count();
    let failures = results.into_iter().filter_map(Result::err).collect();
    Ok(IconRefreshReport {
        total,
        updated,
        failures,
    })
}

#[cfg(test)]
mod project_cache_tests {
    use super::*;
    use crate::workspace::{ProviderChoice, WorkspaceProject};
    use crate::workspace_sources::{RepositoryListMode, SourceRepository, WorkspaceSource};
    use release_desk_core::project::DiskLayout;

    #[test]
    fn clearing_icons_preserves_backup_and_downloads() {
        let project = tempfile::tempdir().unwrap();
        let desk = project.path().join(".release-desk");
        std::fs::create_dir_all(desk.join("icons")).unwrap();
        std::fs::create_dir_all(desk.join("installers")).unwrap();
        std::fs::write(desk.join("data.json"), b"backup").unwrap();
        std::fs::write(desk.join("installers/release.dmg"), b"download").unwrap();
        std::fs::write(desk.join("icons/repo.svg"), b"icon").unwrap();
        std::fs::write(desk.join("agent.db"), b"cache").unwrap();

        clear_project_cache(project.path()).unwrap();

        assert!(!desk.join("icons").exists());
        assert!(!desk.join("agent.db").exists());
        assert_eq!(std::fs::read(desk.join("data.json")).unwrap(), b"backup");
        assert_eq!(
            std::fs::read(desk.join("installers/release.dmg")).unwrap(),
            b"download"
        );
    }

    #[test]
    fn recreating_icons_includes_cloned_unselected_repositories() {
        let project_dir = tempfile::tempdir().unwrap();
        let repo_dir = project_dir.path().join("app");
        std::fs::create_dir_all(repo_dir.join(".git")).unwrap();
        std::fs::create_dir_all(repo_dir.join("assets/app-icon")).unwrap();
        image::RgbaImage::new(512, 512)
            .save(repo_dir.join("assets/app-icon/app-512.png"))
            .unwrap();
        let project = WorkspaceProject {
            id: 1,
            name: "Project".into(),
            directory: project_dir.path().to_path_buf(),
            provider: ProviderChoice::GitHub,
            account: "owner".into(),
            custom_host: None,
            credential_ref: None,
            disk_layout: DiskLayout::Flat,
            sources: vec![WorkspaceSource {
                handle: "owner".into(),
                display_name: None,
                avatar_url: None,
                bio: None,
                provider: "github".into(),
                repositories: vec![SourceRepository {
                    name: "app".into(),
                    full_name: "owner/app".into(),
                    description: None,
                    stars: 0,
                    forks: 0,
                    primary_language: None,
                    is_private: false,
                    is_fork: false,
                    clone_url: String::new(),
                    default_branch: "main".into(),
                    selected: false,
                }],
            }],
            repository_list_mode: RepositoryListMode::Grouped,
        };
        let progress = std::sync::Mutex::new(Vec::new());
        let report = recreate_project_icons_with_progress(&project, |event| {
            progress
                .lock()
                .unwrap()
                .push((event.completed, event.total));
        })
        .unwrap();
        assert_eq!((report.updated, report.total), (1, 1));
        assert!(report.failures.is_empty());
        assert_eq!(*progress.lock().unwrap(), vec![(0, 1), (1, 1)]);
        assert!(resolve_icon_for(Some(project_dir.path()), "github", "owner", "app").is_some());
        let previous =
            resolve_icon_for(Some(project_dir.path()), "github", "owner", "app").unwrap();
        std::fs::remove_file(repo_dir.join("assets/app-icon/app-512.png")).unwrap();
        let failed = recreate_project_icons(&project).unwrap();
        assert_eq!((failed.updated, failed.total), (0, 1));
        assert_eq!(failed.failures.len(), 1);
        assert_eq!(
            resolve_icon_for(Some(project_dir.path()), "github", "owner", "app"),
            Some(previous)
        );
        std::fs::remove_dir_all(repo_dir.join(".git")).unwrap();
        assert!(recreate_project_icons(&project).is_err());
        assert!(resolve_icon_for(Some(project_dir.path()), "github", "owner", "app").is_some());
    }
}
