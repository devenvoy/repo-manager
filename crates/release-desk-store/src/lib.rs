//! Persistent state for Release Desk's first working CLI slice.
//!
//! The legacy snapshot is linked read-only. All writes are confined to the
//! selected managed library and a small pointer in the app config directory.

pub mod archive;
pub mod auth;
pub mod bitbucket_client;
pub mod boundary;
pub mod credentials;
pub mod downloader;
pub mod github_client;
pub mod gitlab_client;
pub mod project_backup;
pub mod provider_releases;
pub mod provider_sources;
pub mod scheduler;
pub mod schema;
pub mod source_fetcher;
pub mod sqlite_store;

pub use archive::TarGzArchivePort;
pub use auth::{
    LocalOAuthServer, ModalWebviewAuthPort, OAuthRedirectOutcome, SystemBrowserAuthPort,
};
pub use bitbucket_client::HttpBitbucketGateway;
pub use boundary::{BoundaryError, ManagedLibrary};
pub use credentials::OsSecretStore;
pub use downloader::HttpDownloadPort;
pub use github_client::HttpGitHubGateway;
pub use gitlab_client::HttpGitLabGateway;
pub use project_backup::{MismatchReport, ProjectSnapshotManager, RevisionMismatch};
pub use scheduler::OsSchedulerPort;
pub use source_fetcher::{
    FetchedProfile, FetchedRepo, fetch_profile_and_repos, parse_input_target,
};
pub use sqlite_store::SqliteStore;

use release_desk_core::{
    Snapshot,
    model::{AccountLogin, RepositoryName},
    parse_snapshot,
};
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};
use thiserror::Error;
use uuid::Uuid;

const LIBRARY_DIR: &str = "Release Desk Library";
const DATABASE_FILE: &str = "release-desk.sqlite3";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("file operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("database operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid snapshot: {0}")]
    Snapshot(#[from] serde_json::Error),
    #[error("settings file is invalid: {0}")]
    Settings(serde_json::Error),
    #[error("Release Desk has not been set up yet; run `release-desk-cli init`")]
    NotInitialized,
    #[error("Release Desk is already set up at {0}")]
    AlreadyInitialized(String),
    #[error("the library must be outside the linked snapshot's source folder")]
    LibraryOverlapsSource,
    #[error("the linked snapshot must be a data.json file inside a .release-desk folder")]
    InvalidSnapshotPath,
    #[error("linked snapshot moved or now points outside its original source folder")]
    SourceChanged,
    #[error("project not found: {0}")]
    ProjectNotFound(String),
    #[error("invalid managed repository path: {0}")]
    InvalidManagedPath(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    library_root: PathBuf,
}

/// A project record independent of the linked snapshot contents.
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub account: String,
    pub linked_snapshot: PathBuf,
}

/// Cached snapshot data. `stale` means the linked source could not be refreshed.
#[derive(Debug)]
pub struct SnapshotView {
    pub snapshot: Snapshot,
    pub stale: bool,
}

pub struct Store {
    connection: Connection,
    library_root: PathBuf,
}

/// Locate the machine-local pointer to the chosen library.
pub fn default_config_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        Ok(PathBuf::from(home).join("Library/Application Support/Release Desk/config.json"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return Ok(PathBuf::from(xdg).join("release-desk/config.json"));
        }
        let home = std::env::var_os("HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        Ok(PathBuf::from(home).join(".config/release-desk/config.json"))
    }
}

impl Store {
    /// Set up a managed library and link one legacy snapshot without modifying it.
    pub fn initialize(library_parent: &Path, snapshot_path: &Path) -> Result<Project> {
        Self::initialize_at(&default_config_path()?, library_parent, snapshot_path)
    }

    /// Variant with an explicit config path for tests and non-default hosts.
    pub fn initialize_at(
        config_path: &Path,
        library_parent: &Path,
        snapshot_path: &Path,
    ) -> Result<Project> {
        if config_path.exists() {
            let config = read_config(config_path)?;
            return Err(StoreError::AlreadyInitialized(
                config.library_root.display().to_string(),
            ));
        }
        let (snapshot_path, source_root) = validate_source(snapshot_path)?;
        let raw = fs::read_to_string(&snapshot_path)?;
        let snapshot = parse_snapshot(&raw)?;

        // Resolve an existing ancestor before creating anything, so an invalid
        // destination inside the linked source folder stays untouched.
        let parent = canonical_future_path(library_parent)?;
        let library_root = parent.join(LIBRARY_DIR);
        if path_overlaps(&library_root, &source_root) {
            return Err(StoreError::LibraryOverlapsSource);
        }
        fs::create_dir_all(&parent)?;
        fs::create_dir_all(&library_root)?;
        let library_root = library_root.canonicalize()?;
        if path_overlaps(&library_root, &source_root) {
            return Err(StoreError::LibraryOverlapsSource);
        }
        let mut connection = open_database(&library_root)?;
        initialize_store_schema(&connection)?;

        let project = Project {
            id: Uuid::new_v4().to_string(),
            name: source_root
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("Imported project")
                .to_string(),
            account: snapshot.org,
            linked_snapshot: snapshot_path,
        };
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO projects (id, name, account) VALUES (?1, ?2, ?3)",
            params![project.id, project.name, project.account],
        )?;
        transaction.execute(
            "INSERT INTO linked_sources (project_id, snapshot_path, source_root) VALUES (?1, ?2, ?3)",
            params![project.id, project.linked_snapshot.to_string_lossy(), source_root.to_string_lossy()],
        )?;
        transaction.execute(
            "INSERT INTO snapshot_cache (project_id, body, generated) VALUES (?1, ?2, ?3)",
            params![project.id, raw, snapshot.generated],
        )?;
        transaction.commit()?;
        write_config(config_path, &AppConfig { library_root })?;
        Ok(project)
    }

    pub fn open() -> Result<Self> {
        Self::open_at(&default_config_path()?)
    }

    pub fn open_at(config_path: &Path) -> Result<Self> {
        if !config_path.exists() {
            return Err(StoreError::NotInitialized);
        }
        let config = read_config(config_path)?;
        let library_root = config.library_root.canonicalize()?;
        let connection = open_database(&library_root)?;
        initialize_store_schema(&connection)?;
        Ok(Self {
            connection,
            library_root,
        })
    }

    pub fn library_root(&self) -> &Path {
        &self.library_root
    }

    /// A clone's only allowed destination inside the managed library.
    pub fn clone_path(&self, project: &Project, repository: &str) -> Result<PathBuf> {
        let account = AccountLogin::new(&project.account)
            .map_err(|error| StoreError::InvalidManagedPath(error.to_string()))?;
        let repository = RepositoryName::new(repository)
            .map_err(|error| StoreError::InvalidManagedPath(error.to_string()))?;
        Uuid::parse_str(&project.id)
            .map_err(|error| StoreError::InvalidManagedPath(error.to_string()))?;
        let path = self
            .library_root
            .join("projects")
            .join(&project.id)
            .join(account.as_str())
            .join(repository.as_str())
            .join("source");
        let resolved = canonical_future_path(&path)?;
        if !resolved.starts_with(&self.library_root) {
            return Err(StoreError::InvalidManagedPath(
                "clone path escapes the library".into(),
            ));
        }
        Ok(path)
    }

    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut statement = self.connection.prepare(
            "SELECT p.id, p.name, p.account, s.snapshot_path \
             FROM projects p JOIN linked_sources s ON s.project_id = p.id ORDER BY p.name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                account: row.get(2)?,
                linked_snapshot: PathBuf::from(row.get::<_, String>(3)?),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn project(&self, id_or_name: &str) -> Result<Project> {
        self.connection
            .query_row(
                "SELECT p.id, p.name, p.account, s.snapshot_path FROM projects p \
                 JOIN linked_sources s ON s.project_id = p.id WHERE p.id = ?1 OR p.name = ?1",
                [id_or_name],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        account: row.get(2)?,
                        linked_snapshot: PathBuf::from(row.get::<_, String>(3)?),
                    })
                },
            )
            .optional()?
            .ok_or_else(|| StoreError::ProjectNotFound(id_or_name.to_string()))
    }

    pub fn cached_snapshot(&self, project: &Project) -> Result<SnapshotView> {
        let (raw, expected_root): (String, String) = self.connection.query_row(
            "SELECT c.body, s.source_root FROM snapshot_cache c \
             JOIN linked_sources s ON s.project_id = c.project_id WHERE c.project_id = ?1",
            [&project.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let snapshot = parse_snapshot(&raw)?;
        let stale = !matches!(
            validate_source(&project.linked_snapshot),
            Ok((source, root)) if source == project.linked_snapshot && root == Path::new(&expected_root)
        );
        Ok(SnapshotView { snapshot, stale })
    }

    /// Read the linked file and update only the managed database cache.
    pub fn refresh_snapshot(&mut self, project: &Project) -> Result<SnapshotView> {
        let expected_root: String = self.connection.query_row(
            "SELECT source_root FROM linked_sources WHERE project_id = ?1",
            [&project.id],
            |row| row.get(0),
        )?;
        let (source, actual_root) = validate_source(&project.linked_snapshot)?;
        if actual_root != Path::new(&expected_root) || source != project.linked_snapshot {
            return Err(StoreError::SourceChanged);
        }
        let raw = fs::read_to_string(source)?;
        let snapshot = parse_snapshot(&raw)?;
        self.connection.execute(
            "UPDATE snapshot_cache SET body = ?2, generated = ?3 WHERE project_id = ?1",
            params![project.id, raw, snapshot.generated],
        )?;
        Ok(SnapshotView {
            snapshot,
            stale: false,
        })
    }
}

fn path_overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

fn canonical_future_path(path: &Path) -> Result<PathBuf> {
    if path.is_relative() {
        return canonical_future_path(&std::env::current_dir()?.join(path));
    }
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "library location has no parent",
        )
    })?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "library location has no name")
    })?;
    Ok(canonical_future_path(parent)?.join(name))
}

fn validate_source(path: &Path) -> Result<(PathBuf, PathBuf)> {
    if path.file_name().is_none_or(|name| name != "data.json")
        || path
            .parent()
            .and_then(Path::file_name)
            .is_none_or(|name| name != ".release-desk")
    {
        return Err(StoreError::InvalidSnapshotPath);
    }
    let canonical = path.canonicalize()?;
    let source_root = canonical
        .parent()
        .and_then(Path::parent)
        .ok_or(StoreError::InvalidSnapshotPath)?
        .to_path_buf();
    Ok((canonical, source_root))
}

fn open_database(root: &Path) -> Result<Connection> {
    let connection = Connection::open(root.join(DATABASE_FILE))?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    Ok(connection)
}

fn initialize_store_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, account TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);\
         CREATE TABLE IF NOT EXISTS linked_sources (project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE, snapshot_path TEXT NOT NULL, source_root TEXT NOT NULL);\
         CREATE TABLE IF NOT EXISTS snapshot_cache (project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE, body TEXT NOT NULL, generated TEXT NOT NULL);",
    )?;
    Ok(())
}

fn read_config(path: &Path) -> Result<AppConfig> {
    serde_json::from_slice(&fs::read(path)?).map_err(StoreError::Settings)
}

fn write_config(path: &Path, config: &AppConfig) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "config path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(config).map_err(StoreError::Settings)?,
    )?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &Path) -> PathBuf {
        let source = dir.join("ArtCraft/.release-desk");
        fs::create_dir_all(&source).expect("source directory");
        let path = source.join("data.json");
        fs::write(
            &path,
            r#"{"org":"storytold","generated":"2026-10-27T00:00:00Z","repos":[]}"#,
        )
        .expect("snapshot");
        path
    }

    #[test]
    fn initialization_links_without_modifying_source() {
        let temp = tempfile::tempdir().expect("temp directory");
        let source = fixture(temp.path());
        let before = fs::read(&source).expect("before");
        let config = temp.path().join("config/config.json");
        let project = Store::initialize_at(&config, &temp.path().join("managed"), &source)
            .expect("initialize");
        assert_eq!(fs::read(&source).expect("after"), before);
        let store = Store::open_at(&config).expect("reopen");
        assert_eq!(store.project("ArtCraft").expect("project").id, project.id);
        assert_eq!(
            store.cached_snapshot(&project).expect("cache").snapshot.org,
            "storytold"
        );
    }

    #[test]
    fn linked_project_uses_the_chosen_source_name() {
        let temp = tempfile::tempdir().expect("temp directory");
        let source = temp.path().join("AnotherProject/.release-desk");
        fs::create_dir_all(&source).expect("source directory");
        let snapshot = source.join("data.json");
        fs::write(
            &snapshot,
            r#"{"org":"owner","generated":"2026-01-01T00:00:00Z","repos":[]}"#,
        )
        .expect("snapshot");
        let project = Store::initialize_at(
            &temp.path().join("config.json"),
            &temp.path().join("library"),
            &snapshot,
        )
        .expect("initialize");
        assert_eq!(project.name, "AnotherProject");
    }

    #[test]
    fn rejects_library_inside_source_folder() {
        let temp = tempfile::tempdir().expect("temp directory");
        let source = fixture(temp.path());
        let config = temp.path().join("config.json");
        let parent = source
            .parent()
            .and_then(Path::parent)
            .expect("ArtCraft")
            .join("new-library");
        assert!(matches!(
            Store::initialize_at(&config, &parent, &source),
            Err(StoreError::LibraryOverlapsSource)
        ));
        assert!(!parent.exists());
    }

    #[test]
    fn linked_file_can_refresh_managed_cache_only() {
        let temp = tempfile::tempdir().expect("temp directory");
        let source = fixture(temp.path());
        let config = temp.path().join("config/config.json");
        let project = Store::initialize_at(&config, &temp.path().join("managed"), &source)
            .expect("initialize");
        fs::write(
            &source,
            r#"{"org":"updated","generated":"2026-10-28T00:00:00Z","repos":[]}"#,
        )
        .expect("external update");
        let mut store = Store::open_at(&config).expect("open");
        assert_eq!(
            store
                .refresh_snapshot(&project)
                .expect("refresh")
                .snapshot
                .org,
            "updated"
        );
        assert_eq!(
            fs::read_to_string(&source).expect("read source"),
            r#"{"org":"updated","generated":"2026-10-28T00:00:00Z","repos":[]}"#
        );
    }
}

pub mod workspace_releases;
