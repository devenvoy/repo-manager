//! End-to-end fake-backed tracer test suite.
//!
//! Exercises the full application lifecycle:
//! 1. Project creation and persistence
//! 2. Repository discovery via GitHub gateway
//! 3. Setting tracking preferences (Inbox -> Tracked / Ignored)
//! 4. Safe Git synchronization (clean, behind, dirty guards)
//! 5. Installer status checking, download, and atomic rotation planning
//! 6. Asynchronous job coordination, event broadcast, and cancellation

use release_desk_application::event::{AppEvent, EventBroadcaster};
use release_desk_application::job_coordinator::JobCoordinator;
use release_desk_application::testing::{
    FakeClock, FakeDownloadPort, FakeGitHubGateway, FakeGitPort, FakeProjectStore,
};
use release_desk_application::use_case::discovery::{
    DiscoverRepositoriesUseCase, InMemoryTrackingStore, SetTrackingPreferencesUseCase,
    TrackingStatus,
};
use release_desk_application::use_case::installer::{
    DownloadAndPlanInstallerUseCase, InstallAssetCommand,
};
use release_desk_application::use_case::project::{
    CreateProjectCommand, CreateProjectUseCase, ListProjectsUseCase,
};
use release_desk_application::use_case::sync::{SyncOutcome, SyncRepositoryUseCase};
use release_desk_core::git::GitSyncState;
use release_desk_core::job::{JobKind, JobPriority};
use release_desk_core::port::ClockPort;
use release_desk_core::project::{
    AccountIdentity, AccountKind, AccountLogin, ProjectId, ProjectName, TrackingPreferences,
};
use release_desk_core::repository::{GitHubRepositoryId, RepositoryIdentity, RepositoryName};
use release_desk_core::storage::{RelativeManagedPath, Sha256Checksum};
use std::path::Path;
use std::sync::Arc;
use tokio::time::Duration;

#[tokio::test]
async fn full_lifecycle_application_tracer() {
    // 0. Initialize ports and infrastructure fakes
    let clock = Arc::new(FakeClock::new(1_700_000_000));
    assert_eq!(clock.now_utc_epoch_secs(), 1_700_000_000);
    clock.advance_seconds(100);
    assert_eq!(clock.now_utc_epoch_secs(), 1_700_000_100);

    let project_store = Arc::new(FakeProjectStore::new());
    let github_gateway = Arc::new(FakeGitHubGateway::new());
    let git_port = Arc::new(FakeGitPort::new());
    let download_port = Arc::new(FakeDownloadPort::new());
    let tracking_store = Arc::new(InMemoryTrackingStore::default());

    let broadcaster = EventBroadcaster::new(128);
    let mut event_rx = broadcaster.subscribe();
    let job_coordinator = JobCoordinator::new(4, broadcaster.clone());

    // 1. Create a Project
    let project_id = ProjectId::from_u128(1001);
    let project_name = ProjectName::new("ArtCraft Studio").expect("valid project name");
    let account = AccountIdentity {
        login: AccountLogin::new("storytold").expect("valid login"),
        kind: AccountKind::Organization,
    };

    let create_project_uc = CreateProjectUseCase::new(project_store.clone());
    let list_projects_uc = ListProjectsUseCase::new(project_store.clone());

    let created_project = create_project_uc
        .execute(CreateProjectCommand {
            id: project_id,
            name: project_name.clone(),
            account: account.clone(),
        })
        .expect("project creation succeeds");

    assert_eq!(created_project.id, project_id);
    assert_eq!(created_project.name, project_name);

    let all_projects = list_projects_uc.execute().expect("list projects");
    assert_eq!(all_projects.len(), 1);
    assert_eq!(all_projects[0].name.as_str(), "ArtCraft Studio");

    // 2. Discover repositories via GitHubGateway
    let repo_core = RepositoryIdentity {
        id: GitHubRepositoryId::from_u64(101),
        owner: account.login.clone(),
        name: RepositoryName::new("artcraft-core").expect("valid repo name"),
    };
    let repo_app = RepositoryIdentity {
        id: GitHubRepositoryId::from_u64(102),
        owner: account.login.clone(),
        name: RepositoryName::new("artcraft-app").expect("valid repo name"),
    };
    let repo_docs = RepositoryIdentity {
        id: GitHubRepositoryId::from_u64(103),
        owner: account.login.clone(),
        name: RepositoryName::new("artcraft-docs").expect("valid repo name"),
    };

    github_gateway.add_repository(account.login.clone(), repo_core.clone());
    github_gateway.add_repository(account.login.clone(), repo_app.clone());
    github_gateway.add_repository(account.login.clone(), repo_docs.clone());

    let discover_uc =
        DiscoverRepositoriesUseCase::new(github_gateway.clone(), tracking_store.clone());
    let discovered = discover_uc
        .execute(project_id, &account)
        .expect("repository discovery succeeds");

    assert_eq!(discovered.len(), 3);
    assert!(discovered.iter().all(|r| r.status == TrackingStatus::Inbox));

    // 3. Configure tracking preferences (Track 2, Ignore 1)
    let set_tracking_uc = SetTrackingPreferencesUseCase::new(tracking_store.clone());

    set_tracking_uc
        .execute(
            project_id,
            repo_core.id,
            TrackingStatus::Tracked(TrackingPreferences::all()),
        )
        .expect("track core");

    set_tracking_uc
        .execute(
            project_id,
            repo_app.id,
            TrackingStatus::Tracked(TrackingPreferences::all()),
        )
        .expect("track app");

    set_tracking_uc
        .execute(project_id, repo_docs.id, TrackingStatus::Ignored)
        .expect("ignore docs");

    let discovered_after = discover_uc
        .execute(project_id, &account)
        .expect("discovery after tracking changes");

    let core_item = discovered_after
        .iter()
        .find(|r| r.identity.id == repo_core.id)
        .unwrap();
    assert_eq!(
        core_item.status,
        TrackingStatus::Tracked(TrackingPreferences::all())
    );

    let docs_item = discovered_after
        .iter()
        .find(|r| r.identity.id == repo_docs.id)
        .unwrap();
    assert_eq!(docs_item.status, TrackingStatus::Ignored);

    // 4. Safe Git Sync across multiple repo states (clean, behind, dirty)
    let path_clean = Path::new("/workspace/artcraft-core");
    let path_behind = Path::new("/workspace/artcraft-app");
    let path_dirty = Path::new("/workspace/artcraft-dirty");

    git_port.set_state(
        path_clean,
        GitSyncState::Current {
            revision: "commit_clean_111".into(),
        },
    );
    git_port.set_state(
        path_behind,
        GitSyncState::Behind {
            local: "commit_local_222".into(),
            remote: "commit_remote_333".into(),
        },
    );
    git_port.set_state(
        path_dirty,
        GitSyncState::Dirty {
            local: "commit_dirty_local".into(),
            remote: "commit_dirty_remote".into(),
        },
    );

    let sync_uc = SyncRepositoryUseCase::new(git_port.clone(), broadcaster.clone());

    // 4a. Sync clean repo -> AlreadyCurrent
    let outcome_clean = sync_uc
        .execute(
            project_id,
            "artcraft-core",
            "https://github.com/storytold/artcraft-core.git",
            path_clean,
            "main",
        )
        .expect("sync clean");
    assert_eq!(outcome_clean, SyncOutcome::AlreadyCurrent);

    // 4b. Sync behind repo -> FastForwarded
    let outcome_behind = sync_uc
        .execute(
            project_id,
            "artcraft-app",
            "https://github.com/storytold/artcraft-app.git",
            path_behind,
            "main",
        )
        .expect("sync behind");
    assert_eq!(
        outcome_behind,
        SyncOutcome::FastForwarded {
            revision: "updated_commit_hash".into(),
        }
    );

    // 4c. Sync dirty repo -> Guarded / Skipped (preserves uncommitted local work)
    let outcome_dirty = sync_uc
        .execute(
            project_id,
            "artcraft-dirty",
            "https://github.com/storytold/artcraft-dirty.git",
            path_dirty,
            "main",
        )
        .expect("sync dirty");
    match outcome_dirty {
        SyncOutcome::Skipped { reason } => {
            assert!(
                reason.contains("uncommitted local changes"),
                "expected uncommitted changes reason, got: {reason}"
            );
        }
        other => panic!("expected dirty clone to be skipped, got: {other:?}"),
    }

    // Verify broadcasted sync events
    let mut sync_events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        if let AppEvent::SyncStarted { repository, .. } = event {
            sync_events.push(repository);
        }
    }
    assert_eq!(
        sync_events,
        vec!["artcraft-core", "artcraft-app", "artcraft-dirty"]
    );

    // 5. Installer checking, download, and atomic rotation planning
    let sample_checksum =
        Sha256Checksum::new("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
            .expect("valid sha256");
    let download_url =
        "https://github.com/storytold/artcraft-app/releases/download/v1.2.0/ArtCraft-v1.2.0.dmg";
    download_port.set_checksum(download_url, sample_checksum.clone());

    let staged_path = RelativeManagedPath::new("staged/ArtCraft-v1.2.0.dmg").expect("valid path");
    let current_file = RelativeManagedPath::new("current/ArtCraft-v1.1.0.dmg").expect("valid path");
    let download_uc = DownloadAndPlanInstallerUseCase::new(download_port.clone());

    let outcome = download_uc
        .execute(InstallAssetCommand {
            download_url: download_url.into(),
            expected_size: 15_000_000,
            expected_checksum: Some(sample_checksum.clone()),
            staged_path: staged_path.clone(),
            current_files: vec![current_file.clone()],
            archived_files: vec![],
            is_same_version: false,
            retention_limit: 3,
        })
        .expect("download and plan rotation");

    assert_eq!(outcome.computed_checksum, sample_checksum);
    assert_eq!(outcome.rotation_plan.to_install, vec![staged_path]);
    assert_eq!(outcome.rotation_plan.to_archive, vec![current_file]);
    assert!(outcome.rotation_plan.to_prune.is_empty());

    // 6. Job coordinator async execution and cancellation
    let job_id = JobCoordinator::next_job_id();
    let coordinator = job_coordinator.clone();

    // Spawn an async job and verify cancellation
    let jh = tokio::spawn(async move {
        coordinator
            .run_job(
                job_id,
                JobKind::DownloadInstaller,
                JobPriority::Normal,
                |_token| async {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    Ok("finished")
                },
            )
            .await
    });

    // Give it a moment to enter run_job, then cancel
    tokio::time::sleep(Duration::from_millis(20)).await;
    let cancelled = job_coordinator.cancel_job(job_id);
    assert!(cancelled, "job was tracked and cancelled");

    let result = jh.await.expect("task join");
    assert!(result.is_err(), "cancelled job yields Err");

    println!("Full lifecycle tracer completed successfully!");
}
