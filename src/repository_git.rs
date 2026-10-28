//! Repository status and guarded Git actions for a project workspace.
//!
//! Call these functions on a background thread: Git and network operations block.

use crate::workspace::WorkspaceProject;
use release_desk_git::{CloneState, CloneStatus, GitClient, GitCredentialContext, SyncOutcome};
use std::path::{Component, Path, PathBuf};

/// A concrete repository and destination resolved from a project.
#[derive(Clone, Debug)]
pub struct RepositoryTarget {
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
}

/// Local state, or an actionable failure. A local inspection is not a remote check.
#[derive(Clone, Debug)]
pub struct RepositoryStatus {
    pub source_index: usize,
    pub repo_index: usize,
    pub state: Result<CloneState, String>,
    pub fetched: bool,
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

/// Resolve selected repositories, rejecting paths that could escape the project.
pub fn targets(project: &WorkspaceProject) -> Result<Vec<RepositoryTarget>, String> {
    let mut result = Vec::new();
    let mut destinations = std::collections::HashSet::new();
    for (source_index, source) in project.sources.iter().enumerate() {
        for (repo_index, repo) in source
            .repositories
            .iter()
            .enumerate()
            .filter(|(_, r)| r.selected)
        {
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

fn git_client(target: &RepositoryTarget) -> Result<GitClient, String> {
    let mut client = GitClient::new();
    if let Some(reference) = &target.credential_ref {
        let executable = std::env::current_exe()
            .map_err(|error| format!("Cannot locate Git credential helper: {error}"))?;
        let credential = GitCredentialContext::new(executable, reference.clone(), &target.url)
            .map_err(|error| error.to_string())?;
        client = client.with_credential(credential);
    }
    Ok(client)
}

/// Clone one selected repository target.
pub fn clone_target(target: &RepositoryTarget) -> Result<SyncOutcome, String> {
    validate_target_path(&target.project_root, &target.path)?;
    git_client(target)?
        .clone_only(&target.url, &target.path, &target.branch)
        .map_err(|error| error.to_string())
}

/// Clone one selected repository after revalidating the project and destination.
/// This never updates an existing directory. Run on a background thread.
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
    RepositoryStatus {
        source_index: target.source_index,
        repo_index: target.repo_index,
        state: validate_target_path(&target.project_root, &target.path).and_then(|()| {
            git_client(target)?
                .inspect(&target.path, &target.branch)
                .map_err(|e| e.to_string())
        }),
        fetched: false,
    }
}

/// Fetch remote refs and inspect; never changes the working tree.
pub fn check_for_updates(target: &RepositoryTarget) -> RepositoryStatus {
    let state = validate_target_path(&target.project_root, &target.path).and_then(|()| {
        git_client(target)?
            .fetch_verified(&target.url, &target.path, &target.branch)
            .map_err(|e| e.to_string())
    });
    let fetched = state
        .as_ref()
        .is_ok_and(|s| !matches!(s.status, CloneStatus::NotCloned | CloneStatus::Error));
    RepositoryStatus {
        source_index: target.source_index,
        repo_index: target.repo_index,
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

#[cfg(test)]
mod tests {
    use super::{safe_component, safe_namespace};
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
}
