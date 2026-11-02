//! Repository status and guarded Git actions for a project workspace.
//!
//! Call these functions on a background thread: Git and network operations block.

use crate::workspace::WorkspaceProject;
pub use release_desk_git::GitCommit;
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
    pub provider: String,
    pub account: String,
    pub endpoint: Option<String>,
}

/// Local state, or an actionable failure. A local inspection is not a remote check.
#[derive(Clone, Debug)]
pub struct RepositoryStatus {
    pub source_index: usize,
    pub repo_index: usize,
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
            GitClient::new()
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
        RepositoryStatus, can_pull, delete_repository_checkout, resolve_target_path,
        safe_component, safe_namespace,
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
}
