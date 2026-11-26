//! Phase 1 -> Phase 3 / Step A verification tests proving the clone flow, atomic cleanup,
//! and per-row destination classifier behavior across every path state.

use release_desk_core::project::DiskLayout;
use release_desk_git::{
    CloneStatus, DestinationKind, GitClient, SymlinkState, SyncOutcome, classify_destination,
    cleanup_stale_partial_clones, format_partial_marker,
};
use repo_manager::repository_git::{
    RepositoryTarget, classify_repo_destination, inspect, is_incomplete_clone,
    remove_incomplete_checkout, targets,
};
use repo_manager::views::project_repository_status::build_repository_presentation;
use repo_manager::workspace::{ProviderChoice, WorkspaceProject};
use repo_manager::workspace_sources::{
    RepoKey, RepositoryListMode, SourceRepository, WorkspaceSource,
};
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git installed");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn make_remote_and_project(root: &Path, repo_name: &str) -> (std::path::PathBuf, WorkspaceProject) {
    let remote = root.join("upstream");
    let project_dir = root.join("project");
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::create_dir_all(&project_dir).unwrap();
    git(&remote, &["init", "-b", "main"]);
    git(&remote, &["config", "user.email", "test@example.invalid"]);
    git(&remote, &["config", "user.name", "Test"]);
    std::fs::write(remote.join("README.md"), "hello\n").unwrap();
    git(&remote, &["add", "README.md"]);
    git(&remote, &["commit", "-m", "initial"]);

    let project = WorkspaceProject {
        id: 101,
        name: "Phase3".into(),
        directory: project_dir,
        provider: ProviderChoice::GitHub,
        account: "owner".into(),
        custom_host: None,
        credential_ref: None,
        disk_layout: DiskLayout::Grouped,
        sources: vec![WorkspaceSource {
            handle: "owner".into(),
            display_name: None,
            avatar_url: None,
            bio: None,
            provider: "github".into(),
            repositories: vec![SourceRepository {
                name: repo_name.into(),
                full_name: format!("owner/{repo_name}"),
                description: None,
                stars: 0,
                forks: 0,
                primary_language: None,
                is_private: false,
                is_fork: false,
                clone_url: remote.to_string_lossy().into_owned(),
                default_branch: "main".into(),
                selected: true,
            }],
        }],
        repository_list_mode: RepositoryListMode::Grouped,
    };
    (remote, project)
}

fn has_partial_dirs(parent: &Path) -> bool {
    std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().contains(".partial"))
}

#[test]
fn phase3_atomic_clone_handles_empty_dir_failure_empty_remote_and_stale_partials() {
    let temp = tempfile::tempdir().unwrap();
    let (_remote, project) = make_remote_and_project(temp.path(), "demo");
    let target = targets(&project).unwrap().remove(0);

    // 1. Pre-existing empty dir is clonable and atomically replaced.
    std::fs::create_dir_all(&target.path).unwrap();
    let kind = classify_destination(&target.project_root, &target.path);
    assert_eq!(kind, DestinationKind::EmptyDir);
    assert!(kind.is_clonable());

    let outcome = GitClient::new()
        .clone_only(&target.url, &target.path, &target.branch)
        .expect("atomic clone succeeds over EmptyDir");
    assert!(matches!(outcome, SyncOutcome::Cloned { .. }));
    assert_eq!(
        classify_destination(&target.project_root, &target.path),
        DestinationKind::GitRepo
    );
    assert!(!has_partial_dirs(target.path.parent().unwrap()));

    // 2. Failing clone never creates destination dir and cleans up its .partial staging dir.
    let failing_path = project.directory.join("owner").join("failing");
    let missing_remote = temp.path().join("does-not-exist");
    let _ = GitClient::new()
        .clone_only(&missing_remote.to_string_lossy(), &failing_path, "main")
        .unwrap_err();
    assert_eq!(
        classify_destination(&project.directory, &failing_path),
        DestinationKind::Missing
    );
    assert!(!has_partial_dirs(failing_path.parent().unwrap()));

    // 3. Empty remote repo (0 commits) completes as Cloned (not Skipped) and inspect sees GitRepo.
    let empty_remote = temp.path().join("empty-upstream");
    std::fs::create_dir_all(&empty_remote).unwrap();
    git(&empty_remote, &["init", "-b", "main"]);
    let empty_clone_path = project
        .directory
        .join("owner")
        .join("empty-remote-checkout");
    let empty_outcome = GitClient::new()
        .clone_only(&empty_remote.to_string_lossy(), &empty_clone_path, "main")
        .unwrap();
    assert!(
        matches!(empty_outcome, SyncOutcome::Cloned { ref revision } if revision.is_empty()),
        "expected empty remote to produce SyncOutcome::Cloned with empty revision, got {empty_outcome:?}"
    );
    assert_eq!(
        classify_destination(&project.directory, &empty_clone_path),
        DestinationKind::GitRepo
    );

    // 4. Stale partial cleanup requires marker + dead PID + minimum age.
    let owner_dir = project.directory.join("owner");
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let stale_partial = owner_dir.join("repo.partial-99999-1");
    std::fs::create_dir_all(stale_partial.join("checkout")).unwrap();
    std::fs::write(
        stale_partial.join(".release-desk-partial"),
        format_partial_marker(999_999, now_secs.saturating_sub(600)),
    )
    .unwrap();

    let user_partial = owner_dir.join("user-created.partial");
    std::fs::create_dir_all(&user_partial).unwrap();
    std::fs::write(user_partial.join("notes.txt"), b"keep").unwrap();

    let removed = cleanup_stale_partial_clones(std::slice::from_ref(&project.directory));
    assert_eq!(removed, 1);
    assert!(!stale_partial.exists());
    assert!(user_partial.exists());
}

#[test]
fn classifier_row_missing() {
    let temp = tempfile::tempdir().unwrap();
    let (_remote, project) = make_remote_and_project(temp.path(), "missing");
    let target = targets(&project).unwrap().remove(0);

    assert_eq!(
        classify_destination(&project.directory, &target.path),
        DestinationKind::Missing
    );
    assert_eq!(
        classify_repo_destination(
            &project.directory,
            project.disk_layout,
            &project.sources[0],
            &project.sources[0].repositories[0],
        ),
        DestinationKind::Missing
    );
    assert_eq!(
        GitClient::new()
            .inspect(&target.path, "main")
            .unwrap()
            .status,
        CloneStatus::NotCloned
    );
    let status = inspect(&target);
    assert_eq!(status.destination, DestinationKind::Missing);
    assert!(!is_incomplete_clone(&status));
    assert!(
        !build_repository_presentation(&project, 0, 0)
            .unwrap()
            .present
    );
}

#[test]
fn classifier_row_empty_dir() {
    let temp = tempfile::tempdir().unwrap();
    let (_remote, project) = make_remote_and_project(temp.path(), "empty");
    let target = targets(&project).unwrap().remove(0);
    std::fs::create_dir_all(&target.path).unwrap();

    assert_eq!(
        classify_destination(&project.directory, &target.path),
        DestinationKind::EmptyDir
    );
    assert_eq!(
        GitClient::new()
            .inspect(&target.path, "main")
            .unwrap()
            .status,
        CloneStatus::Error
    );
    let status = inspect(&target);
    assert_eq!(status.destination, DestinationKind::EmptyDir);
    assert!(is_incomplete_clone(&status));
    assert!(
        !build_repository_presentation(&project, 0, 0)
            .unwrap()
            .present
    );

    remove_incomplete_checkout(&project.directory, &target.path).unwrap();
    assert_eq!(
        classify_destination(&project.directory, &target.path),
        DestinationKind::Missing
    );
}

#[test]
fn classifier_row_non_git_dir() {
    let temp = tempfile::tempdir().unwrap();
    let (_remote, project) = make_remote_and_project(temp.path(), "nongit");
    let target = targets(&project).unwrap().remove(0);
    std::fs::create_dir_all(&target.path).unwrap();
    std::fs::write(target.path.join("notes.txt"), "hi").unwrap();

    assert_eq!(
        classify_destination(&project.directory, &target.path),
        DestinationKind::NonGitDir
    );
    assert_eq!(
        GitClient::new()
            .inspect(&target.path, "main")
            .unwrap()
            .status,
        CloneStatus::Error
    );
    let status = inspect(&target);
    assert_eq!(status.destination, DestinationKind::NonGitDir);
    assert!(is_incomplete_clone(&status));
    assert!(
        !build_repository_presentation(&project, 0, 0)
            .unwrap()
            .present
    );
    // Safety: auto-remove refuses non-git folder containing user content.
    assert!(remove_incomplete_checkout(&project.directory, &target.path).is_err());
    assert!(target.path.join("notes.txt").exists());
}

#[test]
fn classifier_row_valid_repo() {
    let temp = tempfile::tempdir().unwrap();
    let (_remote, project) = make_remote_and_project(temp.path(), "valid");
    let target = targets(&project).unwrap().remove(0);
    GitClient::new()
        .clone_only(&target.url, &target.path, &target.branch)
        .unwrap();

    assert_eq!(
        classify_destination(&project.directory, &target.path),
        DestinationKind::GitRepo
    );
    assert_eq!(
        classify_repo_destination(
            &project.directory,
            project.disk_layout,
            &project.sources[0],
            &project.sources[0].repositories[0],
        ),
        DestinationKind::GitRepo
    );
    assert_eq!(
        GitClient::new()
            .inspect(&target.path, "main")
            .unwrap()
            .status,
        CloneStatus::Current
    );
    let status = inspect(&target);
    assert_eq!(status.destination, DestinationKind::GitRepo);
    assert!(!is_incomplete_clone(&status));
    assert!(
        build_repository_presentation(&project, 0, 0)
            .unwrap()
            .present
    );
}

#[test]
fn classifier_row_wrong_root() {
    let temp = tempfile::tempdir().unwrap();
    let (_remote, project) = make_remote_and_project(temp.path(), "sub");
    let parent_repo = project.directory.join("owner").join("parent");
    std::fs::create_dir_all(&parent_repo).unwrap();
    git(&parent_repo, &["init", "-b", "main"]);
    let nested = parent_repo.join("sub");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("inside.txt"), "nested\n").unwrap();

    assert!(matches!(
        classify_destination(&project.directory, &nested),
        DestinationKind::WrongRoot { .. }
    ));
    assert_eq!(
        GitClient::new().inspect(&nested, "main").unwrap().status,
        CloneStatus::Error
    );
    let status = inspect(&RepositoryTarget {
        key: RepoKey::new("github", "github.com", "owner", "sub"),
        source_index: 0,
        repo_index: 0,
        name: "sub".into(),
        url: "https://github.com/owner/sub.git".into(),
        branch: "main".into(),
        path: nested.clone(),
        project_root: project.directory.clone(),
        credential_ref: None,
        provider: "github".into(),
        account: "owner".into(),
        endpoint: None,
    });
    assert!(matches!(
        status.destination,
        DestinationKind::WrongRoot { .. }
    ));
    assert!(!is_incomplete_clone(&status));
}

#[cfg(unix)]
#[test]
fn classifier_row_broken_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let (_remote, project) = make_remote_and_project(temp.path(), "broken-link");
    std::fs::create_dir_all(project.directory.join("owner")).unwrap();
    let broken_link = project.directory.join("owner").join("broken-link");
    std::os::unix::fs::symlink(temp.path().join("does-not-exist"), &broken_link).unwrap();

    assert_eq!(
        classify_destination(&project.directory, &broken_link),
        DestinationKind::Symlink(SymlinkState::Broken)
    );
    assert_eq!(
        GitClient::new()
            .inspect(&broken_link, "main")
            .unwrap()
            .status,
        CloneStatus::Error
    );
    assert!(
        !build_repository_presentation(&project, 0, 0)
            .unwrap()
            .present
    );
}

#[cfg(unix)]
#[test]
fn classifier_row_valid_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let (remote, mut project) = make_remote_and_project(temp.path(), "real");
    let target = targets(&project).unwrap().remove(0);
    GitClient::new()
        .clone_only(remote.to_str().unwrap(), &target.path, "main")
        .unwrap();

    let valid_link = project.directory.join("owner").join("valid-link");
    std::os::unix::fs::symlink(&target.path, &valid_link).unwrap();
    assert_eq!(
        classify_destination(&project.directory, &valid_link),
        DestinationKind::Symlink(SymlinkState::Valid)
    );
    assert_eq!(
        GitClient::new()
            .inspect(&valid_link, "main")
            .unwrap()
            .status,
        CloneStatus::Error
    );
    project.sources[0].repositories[0].name = "valid-link".into();
    assert!(
        !build_repository_presentation(&project, 0, 0)
            .unwrap()
            .present
    );
    let status = inspect(&RepositoryTarget {
        path: valid_link,
        ..target
    });
    assert_eq!(
        status.destination,
        DestinationKind::Symlink(SymlinkState::Valid)
    );
    assert!(status.state.is_err());
    assert!(!is_incomplete_clone(&status));
}

#[test]
fn classifier_row_unreadable() {
    let temp = tempfile::tempdir().unwrap();
    let (_remote, project) = make_remote_and_project(temp.path(), "outside");
    let status_outside = classify_destination(&project.directory, temp.path());
    assert!(matches!(status_outside, DestinationKind::Unreadable(_)));
}
