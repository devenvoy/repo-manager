//! CLI dependency wiring and runtime context.

use release_desk_application::event::EventBroadcaster;
use release_desk_application::provider_registry::ProviderRegistry;
use release_desk_application::use_case::auth::AuthenticateProviderUseCase;
use release_desk_application::use_case::discovery::{
    DiscoverRepositoriesUseCase, SetTrackingPreferencesUseCase, TrackingStore,
};
use release_desk_application::use_case::project::{CreateProjectUseCase, ListProjectsUseCase};
use release_desk_application::use_case::reconcile::ReconcileProjectSnapshotUseCase;
use release_desk_application::use_case::sync::{SyncAllRepositoriesUseCase, SyncRepositoryUseCase};
use release_desk_core::error::DomainError;
use release_desk_core::port::{
    ArchivePort, BrowserAuthPort, GitPort, ProjectBackupPort, ProjectStore, SecretStorePort,
    WebviewAuthPort,
};
use release_desk_git::SystemGitPort;
use release_desk_store::auth::{ModalWebviewAuthPort, SystemBrowserAuthPort};
use release_desk_store::{
    HttpBitbucketGateway, HttpGitHubGateway, HttpGitLabGateway, ManagedLibrary, OsSecretStore,
    ProjectSnapshotManager, SqliteStore, TarGzArchivePort,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Runtime context holding instantiated adapters and application use cases.
#[allow(dead_code)]
pub struct CliContext {
    pub library: ManagedLibrary,
    pub project_store: Arc<dyn ProjectStore>,
    pub tracking_store: Arc<dyn TrackingStore>,
    pub backup_port: Arc<dyn ProjectBackupPort>,
    pub secret_store: Arc<dyn SecretStorePort>,
    pub git_port: Arc<dyn GitPort>,
    pub archive_port: Arc<dyn ArchivePort>,
    pub browser_auth: Arc<dyn BrowserAuthPort>,
    pub webview_auth: Arc<dyn WebviewAuthPort>,
    pub provider_registry: Arc<ProviderRegistry>,
    pub events: EventBroadcaster,

    // Application use cases
    pub create_project_uc: CreateProjectUseCase,
    pub list_projects_uc: ListProjectsUseCase,
    pub reconcile_uc: ReconcileProjectSnapshotUseCase,
    pub discover_uc: DiscoverRepositoriesUseCase,
    pub track_uc: SetTrackingPreferencesUseCase,
    pub sync_repo_uc: SyncRepositoryUseCase,
    pub sync_all_uc: SyncAllRepositoriesUseCase,
    pub auth_uc: AuthenticateProviderUseCase,
}

impl CliContext {
    /// Initialize runtime context from an optional library root override.
    pub fn new(library_override: Option<&Path>) -> Result<Self, DomainError> {
        let lib_path = resolve_library_path(library_override);
        fs::create_dir_all(&lib_path).map_err(|e| DomainError::PortFailure {
            port: "Filesystem",
            message: format!("cannot create library dir {}: {e}", lib_path.display()),
        })?;

        let library = ManagedLibrary::new(&lib_path).map_err(|e| DomainError::PortFailure {
            port: "LibraryBoundary",
            message: e.to_string(),
        })?;

        let db_path = lib_path.join("release-desk.sqlite3");
        let sqlite = SqliteStore::open(&db_path, library.clone())?;

        let project_store: Arc<dyn ProjectStore> = Arc::new(sqlite.clone());
        let tracking_store: Arc<dyn TrackingStore> = Arc::new(sqlite);
        let backup_port: Arc<dyn ProjectBackupPort> = Arc::new(ProjectSnapshotManager::new());
        let secret_store: Arc<dyn SecretStorePort> = Arc::new(OsSecretStore::default());
        let git_port: Arc<dyn GitPort> = Arc::new(SystemGitPort::new());
        let archive_port: Arc<dyn ArchivePort> = Arc::new(TarGzArchivePort::new());
        let browser_auth: Arc<dyn BrowserAuthPort> = Arc::new(SystemBrowserAuthPort::new());
        let webview_auth: Arc<dyn WebviewAuthPort> = Arc::new(ModalWebviewAuthPort::new());
        let events = EventBroadcaster::default();

        let registry = ProviderRegistry::default();
        let gh_gw = Arc::new(HttpGitHubGateway::new(None));
        let gl_gw = Arc::new(HttpGitLabGateway::new(None));
        let bb_gw = Arc::new(HttpBitbucketGateway::new(None));

        registry.register(gh_gw.clone());
        registry.register(gl_gw);
        registry.register(bb_gw);
        let provider_registry = Arc::new(registry);

        let create_project_uc =
            CreateProjectUseCase::with_backup_port(project_store.clone(), backup_port.clone());
        let list_projects_uc = ListProjectsUseCase::new(project_store.clone());
        let reconcile_uc = ReconcileProjectSnapshotUseCase::new(
            project_store.clone(),
            tracking_store.clone(),
            backup_port.clone(),
        );
        let discover_uc = DiscoverRepositoriesUseCase::new(gh_gw, tracking_store.clone());
        let track_uc = SetTrackingPreferencesUseCase::new(tracking_store.clone());
        let sync_repo_uc = SyncRepositoryUseCase::new(git_port.clone(), events.clone());
        let sync_all_uc = SyncAllRepositoriesUseCase::new(git_port.clone(), events.clone());
        let auth_uc = AuthenticateProviderUseCase::new(
            secret_store.clone(),
            browser_auth.clone(),
            webview_auth.clone(),
            events.clone(),
        );

        Ok(Self {
            library,
            project_store,
            tracking_store,
            backup_port,
            secret_store,
            git_port,
            archive_port,
            browser_auth,
            webview_auth,
            provider_registry,
            events,
            create_project_uc,
            list_projects_uc,
            reconcile_uc,
            discover_uc,
            track_uc,
            sync_repo_uc,
            sync_all_uc,
            auth_uc,
        })
    }
}

/// Determine the managed library root directory.
pub fn resolve_library_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
    }

    if let Ok(env_path) = std::env::var("RELEASE_DESK_LIBRARY") {
        let p = PathBuf::from(env_path);
        return if p.is_absolute() {
            p
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(p)
        };
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".release-desk/library");
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".release-desk-library")
}
