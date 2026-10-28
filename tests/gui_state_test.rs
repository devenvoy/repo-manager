//! Tests for Desktop GUI state, routing, repository filtering, and theme models.

use release_desk_core::git::{BranchRef, Revision};
use release_desk_core::project::{ProjectId, ProjectIdentity, ProjectName};
use release_desk_core::snapshot::{MismatchReport, Repository, RevisionMismatch};
use repo_manager::settings::{self, Appearance, Settings};
use repo_manager::state::{DesktopState, WorkspaceTab};
use std::path::PathBuf;

#[test]
fn test_workspace_tab_titles() {
    assert_eq!(WorkspaceTab::AppStore.title(), "App Store");
    assert_eq!(WorkspaceTab::ProjectHub.title(), "Project Hub");
    assert_eq!(WorkspaceTab::Overview.title(), "Overview");
    assert_eq!(WorkspaceTab::Repositories.title(), "Repositories");
    assert_eq!(WorkspaceTab::Installers.title(), "Installers");
    assert_eq!(WorkspaceTab::PullRequests.title(), "Pull Requests");
    assert_eq!(WorkspaceTab::Reconcile.title(), "Backup & Reconcile");
    assert_eq!(WorkspaceTab::Settings.title(), "Settings");
    assert_eq!(WorkspaceTab::Welcome.title(), "Get Started");
}

#[test]
fn test_desktop_state_default() {
    let state = DesktopState::default();
    assert_eq!(state.active_tab, WorkspaceTab::AppStore);
    assert!(state.active_project.is_none());
    assert!(state.projects.is_empty());
    assert!(state.repositories.is_empty());
    assert!(state.selected_repo_index.is_none());
    assert_eq!(state.search_query, "");
    assert!(state.mismatch_report.is_none());
    assert_eq!(state.status_message, "Ready");
    assert!(state.library_path.is_none());
}

#[test]
fn test_desktop_state_filtering() {
    let mut state = DesktopState::default();

    let make_branch = |name: &str, oid: &str| BranchRef {
        name: name.to_string(),
        target: Revision {
            oid: oid.to_string(),
        },
    };

    let repo1 = Repository {
        name: "StoryTold".to_string(),
        url: "https://github.com/example/StoryTold".to_string(),
        description: Some("Interactive story-telling game engine".to_string()),
        is_private: false,
        is_archived: false,
        pushed_at: None,
        primary_language: None,
        default_branch_ref: Some(make_branch("main", "111111")),
        releases: None,
    };

    let repo2 = Repository {
        name: "AudioSync".to_string(),
        url: "https://github.com/example/AudioSync".to_string(),
        description: Some("Low-latency audio streaming library".to_string()),
        is_private: true,
        is_archived: false,
        pushed_at: None,
        primary_language: None,
        default_branch_ref: Some(make_branch("main", "222222")),
        releases: None,
    };

    let repo3 = Repository {
        name: "ArtCraft-Legacy".to_string(),
        url: "https://github.com/example/ArtCraft-Legacy".to_string(),
        description: None,
        is_archived: true,
        is_private: false,
        pushed_at: None,
        primary_language: None,
        default_branch_ref: Some(make_branch("master", "333333")),
        releases: None,
    };

    state.repositories = vec![repo1, repo2, repo3];

    // Empty search query returns all
    assert_eq!(state.filtered_repositories().len(), 3);

    // Filter by name (case-insensitive)
    state.search_query = "story".to_string();
    let filtered = state.filtered_repositories();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "StoryTold");

    // Filter by description
    state.search_query = "streaming".to_string();
    let filtered = state.filtered_repositories();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "AudioSync");

    // Filter matching nothing
    state.search_query = "nonexistent".to_string();
    assert!(state.filtered_repositories().is_empty());
}

#[test]
fn test_desktop_state_reconciliation_report() {
    let mut state = DesktopState::default();

    // No report initially
    assert!(state.mismatch_report.is_none());

    // Synchronized report
    let sync_report = MismatchReport {
        project_root: PathBuf::from("/tmp/repo"),
        disk_snapshot_present: true,
        matched_repositories: vec!["repo-1".to_string()],
        missing_in_database: Vec::new(),
        missing_on_disk: Vec::new(),
        revision_mismatches: Vec::new(),
    };
    assert!(sync_report.is_synchronized());
    state.mismatch_report = Some(sync_report);
    assert!(state.mismatch_report.as_ref().unwrap().is_synchronized());

    // Divergent report
    let mismatch = MismatchReport {
        project_root: PathBuf::from("/tmp/repo"),
        disk_snapshot_present: true,
        matched_repositories: vec!["repo-1".to_string()],
        missing_in_database: vec!["repo-alpha".to_string()],
        missing_on_disk: Vec::new(),
        revision_mismatches: vec![RevisionMismatch {
            repository: "repo-beta".to_string(),
            disk_revision: Some("def222".to_string()),
            db_revision: Some("abc111".to_string()),
        }],
    };
    assert!(!mismatch.is_synchronized());
    state.mismatch_report = Some(mismatch);
    assert!(!state.mismatch_report.as_ref().unwrap().is_synchronized());
}

#[test]
fn test_desktop_project_navigation() {
    let mut state = DesktopState::default();

    let project = ProjectIdentity::with_clone_location(
        ProjectId::from_u128(1),
        ProjectName::new("TestProject").unwrap(),
        Some(PathBuf::from("/tmp/test_project_clone")),
    );

    state.active_project = Some(project.clone());
    state.projects.push(project);
    state.active_tab = WorkspaceTab::Repositories;
    state.selected_repo_index = Some(2);

    assert_eq!(state.active_tab, WorkspaceTab::Repositories);
    assert_eq!(state.selected_repo_index, Some(2));
    assert_eq!(state.projects.len(), 1);
    assert_eq!(
        state.active_project.as_ref().unwrap().name.as_str(),
        "TestProject"
    );
    assert_eq!(
        state.active_project.as_ref().unwrap().clone_location,
        Some(PathBuf::from("/tmp/test_project_clone"))
    );
}

#[test]
fn test_settings_extension_normalization() {
    assert_eq!(settings::normalized_extension("dmg").unwrap(), "dmg");
    assert_eq!(settings::normalized_extension(".dmg").unwrap(), "dmg");
    assert_eq!(settings::normalized_extension("pkg").unwrap(), "pkg");
    assert_eq!(settings::normalized_extension(".pkg").unwrap(), "pkg");
    assert_eq!(settings::normalized_extension("zip").unwrap(), "zip");

    assert!(settings::normalized_extension("").is_err());
    assert!(settings::normalized_extension("   ").is_err());
    assert!(settings::normalized_extension(".").is_err());
    assert!(settings::normalized_extension("../zip").is_err());

    let default_settings = Settings::default();
    assert_eq!(default_settings.appearance, Appearance::Light);
    assert_eq!(default_settings.installer_extension, "dmg");
    assert!(default_settings.include_prereleases);
    assert!(!default_settings.show_archived);
}

#[test]
fn test_no_unicode_dots_or_checkbox_glyphs() {
    for tab in &[
        WorkspaceTab::AppStore,
        WorkspaceTab::ProjectHub,
        WorkspaceTab::Overview,
        WorkspaceTab::Repositories,
        WorkspaceTab::Installers,
        WorkspaceTab::PullRequests,
        WorkspaceTab::Reconcile,
        WorkspaceTab::Settings,
        WorkspaceTab::Welcome,
    ] {
        assert!(!tab.title().contains('●'));
        assert!(!tab.title().contains('☑'));
        assert!(!tab.title().contains('☐'));
    }
}

#[test]
fn test_find_saved_accounts_for_provider() {
    use release_desk_core::project::DiskLayout;
    use repo_manager::workspace::{
        ProviderChoice, WorkspaceProject, find_saved_accounts_for_provider,
    };
    use std::path::PathBuf;

    let projects = vec![
        WorkspaceProject {
            id: 1,
            name: "Alpha".into(),
            directory: PathBuf::from("/tmp/alpha"),
            provider: ProviderChoice::GitHub,
            account: "octocat".into(),
            custom_host: None,
            credential_ref: Some("release_desk_github_octocat".into()),
            disk_layout: DiskLayout::Grouped,
            sources: vec![],
            repository_list_mode: Default::default(),
        },
        WorkspaceProject {
            id: 2,
            name: "Beta".into(),
            directory: PathBuf::from("/tmp/beta"),
            provider: ProviderChoice::GitHub,
            account: "octocat".into(),
            custom_host: None,
            credential_ref: Some("release_desk_github_octocat".into()),
            disk_layout: DiskLayout::Grouped,
            sources: vec![],
            repository_list_mode: Default::default(),
        },
        WorkspaceProject {
            id: 3,
            name: "Gamma".into(),
            directory: PathBuf::from("/tmp/gamma"),
            provider: ProviderChoice::GitLab,
            account: "gitlab-user".into(),
            custom_host: None,
            credential_ref: Some("release_desk_gitlab_gitlab-user".into()),
            disk_layout: DiskLayout::Flat,
            sources: vec![],
            repository_list_mode: Default::default(),
        },
    ];

    let github_accounts = find_saved_accounts_for_provider(&projects, ProviderChoice::GitHub);
    assert_eq!(github_accounts.len(), 1);
    assert_eq!(github_accounts[0].handle, "octocat");
    assert_eq!(
        github_accounts[0].credential_ref.as_deref(),
        Some("release_desk_github_octocat")
    );
    assert!(github_accounts[0].has_keychain_secret);

    let gitlab_accounts = find_saved_accounts_for_provider(&projects, ProviderChoice::GitLab);
    assert_eq!(gitlab_accounts.len(), 1);
    assert_eq!(gitlab_accounts[0].handle, "gitlab-user");

    let bitbucket_accounts = find_saved_accounts_for_provider(&projects, ProviderChoice::Bitbucket);
    assert!(bitbucket_accounts.is_empty());
}

#[test]
fn test_provider_choice_conversion_and_str() {
    use release_desk_core::provider::GitProviderKind;
    use repo_manager::workspace::ProviderChoice;

    assert_eq!(ProviderChoice::GitHub.as_str(), "github");
    assert_eq!(ProviderChoice::GitLab.as_str(), "gitlab");
    assert_eq!(ProviderChoice::Bitbucket.as_str(), "bitbucket");

    assert_eq!(
        GitProviderKind::from(ProviderChoice::GitHub),
        GitProviderKind::GitHub
    );
    assert_eq!(
        GitProviderKind::from(ProviderChoice::GitLab),
        GitProviderKind::GitLab
    );
    assert_eq!(
        GitProviderKind::from(ProviderChoice::Bitbucket),
        GitProviderKind::Bitbucket
    );
}

#[test]
fn test_oauth_auth_use_case_with_browser_and_webview_fallback() {
    use release_desk_application::event::EventBroadcaster;
    use release_desk_application::testing::FakeSecretStore;
    use release_desk_application::use_case::auth::{
        AuthResult, AuthenticateCommand, AuthenticateProviderUseCase, OAuthConfig,
    };
    use release_desk_core::port::SecretStorePort;
    use release_desk_core::project::AccountLogin;
    use release_desk_core::provider::{AuthMethod, GitProviderKind};
    use release_desk_store::auth::{ModalWebviewAuthPort, SystemBrowserAuthPort};
    use std::sync::Arc;

    let secret_store = Arc::new(FakeSecretStore::new());
    // SystemBrowserAuthPort::new_simulating_missing() ensures fallback to ModalWebviewAuthPort!
    let browser = Arc::new(SystemBrowserAuthPort::new_simulating_missing());
    let webview = Arc::new(ModalWebviewAuthPort::new());
    let events = EventBroadcaster::default();
    let auth_uc = AuthenticateProviderUseCase::new(secret_store.clone(), browser, webview, events);

    let oauth = OAuthConfig {
        client_id: "test-client".into(),
        auth_url: "https://github.com/login/oauth/authorize".into(),
        token_url: "https://github.com/login/oauth/access_token".into(),
        redirect_uri: "http://127.0.0.1:4567/callback".into(),
        scopes: vec!["repo".into()],
    };

    let cmd = AuthenticateCommand {
        provider: GitProviderKind::GitHub,
        account_login: AccountLogin::new("octocat").unwrap(),
        method: AuthMethod::OAuthBrowser,
        oauth_config: Some(oauth),
    };

    let result = auth_uc.execute(cmd).expect("auth execution should return");
    assert!(matches!(result, AuthResult::Failed { error, .. } if error.contains("exchange")));
    assert!(
        secret_store
            .get_secret("release_desk_github_octocat")
            .unwrap()
            .is_none()
    );
}
