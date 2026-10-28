//! Exercises the desktop repository workflow against a real local Git remote.
use release_desk_core::project::DiskLayout;
use release_desk_git::{CloneStatus, SyncOutcome};
use repo_manager::repository_git::{check_for_updates, clone_selected, targets, update};
use repo_manager::workspace::{ProviderChoice, WorkspaceProject};
use repo_manager::workspace_sources::{RepositoryListMode, SourceRepository, WorkspaceSource};
use std::{path::Path, process::Command};
use tempfile::TempDir;

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

fn fixture() -> (TempDir, WorkspaceProject, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let remote = temp.path().join("upstream");
    let folder = temp.path().join("project");
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    git(&remote, &["init", "-b", "main"]);
    git(&remote, &["config", "user.email", "test@example.invalid"]);
    git(&remote, &["config", "user.name", "Test"]);
    std::fs::write(remote.join("file.txt"), "first\n").unwrap();
    git(&remote, &["add", "file.txt"]);
    git(&remote, &["commit", "-m", "first"]);
    let repo = SourceRepository {
        name: "repo".into(),
        full_name: "owner/repo".into(),
        description: None,
        stars: 0,
        forks: 0,
        primary_language: None,
        is_private: false,
        is_fork: false,
        clone_url: remote.to_string_lossy().into_owned(),
        default_branch: "main".into(),
        selected: true,
    };
    let source = WorkspaceSource {
        handle: "owner".into(),
        display_name: None,
        avatar_url: None,
        bio: None,
        provider: "github".into(),
        repositories: vec![repo],
    };
    let project = WorkspaceProject {
        id: 1,
        name: "Project".into(),
        directory: folder,
        provider: ProviderChoice::GitHub,
        account: "owner".into(),
        custom_host: None,
        credential_ref: None,
        disk_layout: DiskLayout::Grouped,
        sources: vec![source],
        repository_list_mode: RepositoryListMode::Grouped,
    };
    (temp, project, remote)
}

#[test]
fn clone_check_and_update_preserve_local_changes() {
    let (_temp, project, remote) = fixture();
    let target = targets(&project).unwrap().remove(0);
    assert_eq!(
        check_for_updates(&target).state.unwrap().status,
        CloneStatus::NotCloned
    );
    assert!(matches!(
        clone_selected(&project, 0, 0).unwrap(),
        SyncOutcome::Cloned { .. }
    ));
    assert!(clone_selected(&project, 0, 0).is_err());
    let before = git(&target.path, &["rev-parse", "HEAD"]);
    std::fs::write(remote.join("file.txt"), "second\n").unwrap();
    git(&remote, &["commit", "-am", "second"]);
    let status = check_for_updates(&target);
    assert!(status.fetched);
    assert_eq!(status.state.unwrap().status, CloneStatus::Behind);
    assert_eq!(
        git(&target.path, &["rev-parse", "HEAD"]),
        before,
        "checking should never update the working tree"
    );
    std::fs::write(target.path.join("local.txt"), "keep me").unwrap();
    assert!(matches!(update(&target).unwrap(), SyncOutcome::Skipped(_)));
    assert_eq!(
        std::fs::read_to_string(target.path.join("local.txt")).unwrap(),
        "keep me"
    );
    assert_eq!(git(&target.path, &["rev-parse", "HEAD"]), before);
    std::fs::remove_file(target.path.join("local.txt")).unwrap();
    assert!(matches!(
        update(&target).unwrap(),
        SyncOutcome::FastForwarded { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(target.path.join("file.txt")).unwrap(),
        "second\n"
    );
}

#[test]
fn selection_and_path_collisions_are_checked_before_writing() {
    let (_temp, mut project, _) = fixture();
    project.sources[0].repositories[0].selected = false;
    assert!(targets(&project).unwrap().is_empty());
    assert!(clone_selected(&project, 0, 0).is_err());
    project.sources[0].repositories[0].selected = true;
    let duplicate = project.sources[0].repositories[0].clone();
    project.sources[0].repositories.push(duplicate);
    assert!(targets(&project).is_err());
    assert!(!project.directory.join("owner").exists());
}

#[test]
fn unexpected_origin_is_not_fetched_or_updated() {
    let (_temp, mut project, remote) = fixture();
    clone_selected(&project, 0, 0).unwrap();
    let target = targets(&project).unwrap().remove(0);
    let before = git(&target.path, &["rev-parse", "HEAD"]);
    let unexpected = remote.parent().unwrap().join("different");
    std::fs::create_dir(&unexpected).unwrap();
    project.sources[0].repositories[0].clone_url = unexpected.to_string_lossy().into_owned();
    let mismatched = targets(&project).unwrap().remove(0);
    let checked = check_for_updates(&mismatched);
    assert!(!checked.fetched);
    assert_eq!(checked.state.unwrap().status, CloneStatus::Error);
    assert!(matches!(
        update(&mismatched).unwrap(),
        SyncOutcome::Skipped(_)
    ));
    assert_eq!(git(&target.path, &["rev-parse", "HEAD"]), before);
}

#[cfg(unix)]
#[test]
fn symlinked_owner_cannot_redirect_clones_outside_project() {
    let (_temp, project, remote) = fixture();
    let outside = remote.parent().unwrap().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, project.directory.join("owner")).unwrap();
    assert!(targets(&project).is_err());
    assert!(clone_selected(&project, 0, 0).is_err());
    assert!(!outside.join("repo").exists());
}

#[cfg(unix)]
#[test]
fn stale_target_refuses_symlink_added_after_resolution() {
    let (_temp, project, remote) = fixture();
    let target = targets(&project).unwrap().remove(0);
    let outside = remote.parent().unwrap().join("outside-stale");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, project.directory.join("owner")).unwrap();

    assert!(
        repo_manager::repository_git::inspect(&target)
            .state
            .is_err()
    );
    assert!(check_for_updates(&target).state.is_err());
    assert!(update(&target).is_err());
    assert!(!outside.join("repo").exists());
}
