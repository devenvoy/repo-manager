//! End-to-end integration tests using real SQLite database files and disk storage.

use release_desk_application::use_case::discovery::{TrackingStatus, TrackingStore};
use release_desk_application::use_case::pull_request::PullRequestStore;
use release_desk_core::job::{IdempotencyKey, JobKind, JobPriority, JobState};
use release_desk_core::port::{ArchivePort, JobStore, ProjectStore};
use release_desk_core::project::{ProjectId, ProjectIdentity, ProjectName, TrackingPreferences};
use release_desk_core::pull_request::{PullRequest, PullRequestState};
use release_desk_core::repository::GitHubRepositoryId;
use release_desk_core::storage::RelativeManagedPath;
use release_desk_store::archive::TarGzArchivePort;
use release_desk_store::boundary::ManagedLibrary;
use release_desk_store::sqlite_store::SqliteStore;
use std::fs;

#[test]
fn real_sqlite_and_storage_lifecycle_integration() {
    let temp = tempfile::tempdir().unwrap();
    let library_dir = temp.path().join("ManagedLibrary");
    fs::create_dir(&library_dir).unwrap();

    let library = ManagedLibrary::new(&library_dir).expect("valid library root");
    let db_path = library_dir.join("release-desk.sqlite3");

    // 1. Open SQLite database on disk (runs migrations automatically)
    let store = SqliteStore::open(&db_path, library.clone()).expect("open sqlite store");
    assert!(db_path.exists());

    // 2. Project lifecycle via ProjectStore
    let project_id = ProjectId::from_u128(999);
    let project_name = ProjectName::new("Storytold Infrastructure").unwrap();
    let project = ProjectIdentity::new(project_id, project_name.clone());

    store.save_project(&project).expect("save project");

    let loaded = store
        .get_project(project_id)
        .expect("get project")
        .expect("project exists");
    assert_eq!(loaded.id, project_id);
    assert_eq!(loaded.name, project_name);

    let projects = store.list_projects().expect("list projects");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name.as_str(), "Storytold Infrastructure");

    // 3. Tracking Preferences via TrackingStore
    let repo_id = GitHubRepositoryId::from_u64(404);
    assert_eq!(
        store.get_preferences(project_id, repo_id).unwrap(),
        TrackingStatus::Inbox
    );

    let prefs = TrackingPreferences {
        source: true,
        installers: true,
        pull_requests: false,
    };
    store
        .set_preferences(project_id, repo_id, TrackingStatus::Tracked(prefs))
        .expect("set tracking");

    assert_eq!(
        store.get_preferences(project_id, repo_id).unwrap(),
        TrackingStatus::Tracked(prefs)
    );

    // 4. Job queueing and idempotency via JobStore
    let idem_key = IdempotencyKey::new("sync_meta_project_999").unwrap();
    let job_id1 = store
        .enqueue_job(JobKind::SyncMetadata, JobPriority::High, &idem_key)
        .expect("enqueue job");

    // Enqueue again with same idempotency key -> returns existing job_id
    let job_id2 = store
        .enqueue_job(JobKind::SyncMetadata, JobPriority::High, &idem_key)
        .expect("enqueue duplicate job");
    assert_eq!(job_id1, job_id2);

    store
        .update_job_state(job_id1, JobState::Completed)
        .expect("update job state");

    // 5. Pull Request storage via PullRequestStore
    let pr = PullRequest {
        number: 42,
        title: "Add SQLite WAL mode".into(),
        state: PullRequestState::Merged,
        draft: false,
        author: None,
        head_ref: "feat/wal".into(),
        base_ref: "main".into(),
        html_url: "https://github.com/storytold/app/pull/42".into(),
        created_at: "2026-10-27T00:00:00Z".into(),
        updated_at: "2026-10-27T01:00:00Z".into(),
        merged_at: Some("2026-10-27T01:00:00Z".into()),
        labels: vec![],
    };

    store
        .save_pull_requests(repo_id, std::slice::from_ref(&pr))
        .expect("save PR");
    let prs = store.get_pull_requests(repo_id).expect("get PRs");
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].number, 42);
    assert_eq!(prs[0].state, PullRequestState::Merged);

    // 6. ManagedLibrary atomic file write
    let rel_file = RelativeManagedPath::new("installers/storytold-app-1.0.0.dmg").unwrap();
    let file_path = library.write_file(&rel_file, b"installer payload").unwrap();
    assert!(file_path.exists());

    // 7. TarGz Archive backup and restore
    let archiver = TarGzArchivePort::new();
    let backup_file = temp.path().join("backup.tar.gz");
    let size = archiver.create_archive(&library_dir, &backup_file).unwrap();
    assert!(size > 0);

    let restore_dir = temp.path().join("RestoredLibrary");
    archiver
        .extract_archive(&backup_file, &restore_dir)
        .unwrap();
    assert!(restore_dir.join("release-desk.sqlite3").exists());
    assert!(
        restore_dir
            .join("installers/storytold-app-1.0.0.dmg")
            .exists()
    );
}
