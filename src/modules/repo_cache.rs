//! Versioned SQLite cache (WAL mode) on the internal disk for instant Repositories paint
//! and incremental fingerprint-based revalidation. Never stores tokens or credentials.

use crate::repository_git::{DestinationKind, RepositoryStatus, SymlinkState};
use crate::settings::default_library_dir_string;
use crate::views::project_repository_status::RepositoryPresentation;
use crate::workspace_sources::RepoKey;
use release_desk_git::{CloneState, CloneStatus, FileStamp, GitCommit, RepoFingerprint};
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_SCHEMA_VERSION: i32 = 3;
const CACHE_FILENAME: &str = "repo_cache.sqlite3";

static TRACE_GIT_SUBPROCESSES: AtomicUsize = AtomicUsize::new(0);
static TRACE_PROJECT_FS_ACCESSES: AtomicUsize = AtomicUsize::new(0);
static TRACE_CACHE_READS: AtomicUsize = AtomicUsize::new(0);
static TRACE_CACHE_WRITES: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static THREAD_BASE_GIT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static THREAD_PROJECT_FS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TraceSnapshot {
    pub git_subprocesses: usize,
    pub project_fs_accesses: usize,
    pub cache_reads: usize,
    pub cache_writes: usize,
}

pub fn reset_trace_counters() {
    TRACE_GIT_SUBPROCESSES.store(
        release_desk_git::perf::total_subprocesses(),
        Ordering::SeqCst,
    );
    TRACE_PROJECT_FS_ACCESSES.store(0, Ordering::SeqCst);
    TRACE_CACHE_READS.store(0, Ordering::SeqCst);
    TRACE_CACHE_WRITES.store(0, Ordering::SeqCst);
    THREAD_BASE_GIT.with(|c| c.set(release_desk_git::perf::thread_subprocesses()));
    THREAD_PROJECT_FS.with(|c| c.set(0));
}

pub fn thread_trace_snapshot() -> TraceSnapshot {
    let base = THREAD_BASE_GIT.with(std::cell::Cell::get);
    let now = release_desk_git::perf::thread_subprocesses();
    TraceSnapshot {
        git_subprocesses: now.saturating_sub(base),
        project_fs_accesses: THREAD_PROJECT_FS.with(std::cell::Cell::get),
        cache_reads: TRACE_CACHE_READS.load(Ordering::SeqCst),
        cache_writes: TRACE_CACHE_WRITES.load(Ordering::SeqCst),
    }
}

pub fn trace_snapshot() -> TraceSnapshot {
    let base = TRACE_GIT_SUBPROCESSES.load(Ordering::SeqCst);
    let now = release_desk_git::perf::total_subprocesses();
    TraceSnapshot {
        git_subprocesses: now.saturating_sub(base),
        project_fs_accesses: TRACE_PROJECT_FS_ACCESSES.load(Ordering::SeqCst),
        cache_reads: TRACE_CACHE_READS.load(Ordering::SeqCst),
        cache_writes: TRACE_CACHE_WRITES.load(Ordering::SeqCst),
    }
}

pub fn record_git_subprocess() {
    TRACE_GIT_SUBPROCESSES.fetch_add(1, Ordering::Relaxed);
}

pub fn record_project_fs_access() {
    TRACE_PROJECT_FS_ACCESSES.fetch_add(1, Ordering::Relaxed);
    THREAD_PROJECT_FS.with(|c| c.set(c.get() + 1));
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct StoredFileStamp {
    pub mtime_secs: u64,
    pub mtime_nanos: u32,
    pub len: u64,
}

impl From<&FileStamp> for StoredFileStamp {
    fn from(v: &FileStamp) -> Self {
        Self {
            mtime_secs: v.mtime_secs,
            mtime_nanos: v.mtime_nanos,
            len: v.len,
        }
    }
}

impl From<&StoredFileStamp> for FileStamp {
    fn from(v: &StoredFileStamp) -> Self {
        Self {
            mtime_secs: v.mtime_secs,
            mtime_nanos: v.mtime_nanos,
            len: v.len,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct StoredFingerprint {
    pub gitdir: PathBuf,
    pub commondir: PathBuf,
    pub branch: Option<String>,
    pub is_reftable: bool,
    pub head_stamp: Option<StoredFileStamp>,
    pub branch_ref_stamp: Option<StoredFileStamp>,
    pub packed_refs_stamp: Option<StoredFileStamp>,
    pub remote_ref_stamp: Option<StoredFileStamp>,
    pub remote_dir_stamp: Option<StoredFileStamp>,
    pub fetch_head_stamp: Option<StoredFileStamp>,
    pub reftable_tables_stamp: Option<StoredFileStamp>,
}

impl From<&RepoFingerprint> for StoredFingerprint {
    fn from(fp: &RepoFingerprint) -> Self {
        Self {
            gitdir: fp.gitdir.clone(),
            commondir: fp.commondir.clone(),
            branch: fp.branch.clone(),
            is_reftable: fp.is_reftable,
            head_stamp: fp.head_stamp.as_ref().map(Into::into),
            branch_ref_stamp: fp.branch_ref_stamp.as_ref().map(Into::into),
            packed_refs_stamp: fp.packed_refs_stamp.as_ref().map(Into::into),
            remote_ref_stamp: fp.remote_ref_stamp.as_ref().map(Into::into),
            remote_dir_stamp: fp.remote_dir_stamp.as_ref().map(Into::into),
            fetch_head_stamp: fp.fetch_head_stamp.as_ref().map(Into::into),
            reftable_tables_stamp: fp.reftable_tables_stamp.as_ref().map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct StoredCommit {
    pub hash: String,
    pub short_hash: String,
    pub author_name: String,
    pub date: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct StoredReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ProbeRepoIssue {
    NotFound,
    AccessLost,
    MovedOrRenamed(String),
    Archived,
    EmptyRepo,
    DefaultBranchRenamed(String),
    HistoryRewritten,
    Offline,
}

impl ProbeRepoIssue {
    pub fn badge_label(&self) -> &'static str {
        match self {
            Self::NotFound => "Not found",
            Self::AccessLost => "Access lost",
            Self::MovedOrRenamed(_) => "Moved / renamed",
            Self::Archived => "Archived",
            Self::EmptyRepo => "Empty repo",
            Self::DefaultBranchRenamed(_) => "Branch renamed",
            Self::HistoryRewritten => "History rewritten",
            Self::Offline => "Offline",
        }
    }

    pub fn detail_message(&self) -> String {
        match self {
            Self::NotFound => "Remote repository could not be found.".into(),
            Self::AccessLost => "Access to this repository was revoked or denied.".into(),
            Self::MovedOrRenamed(target) => {
                if target.is_empty() {
                    "Repository appears to have moved or been renamed.".into()
                } else {
                    format!("Repository moved/renamed to {target}.")
                }
            }
            Self::Archived => "Upstream repository is archived (read-only).".into(),
            Self::EmptyRepo => "Upstream repository has no commits on its default branch.".into(),
            Self::DefaultBranchRenamed(branch) => {
                format!("Default branch was renamed upstream to '{branch}'.")
            }
            Self::HistoryRewritten => {
                "Upstream branch history appears force-pushed or rewritten.".into()
            }
            Self::Offline => "Offline — remote check skipped until connectivity returns.".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedRepoRecord {
    pub project_id: u128,
    pub key: RepoKey,
    pub destination: DestinationKind,
    pub status: CloneStatus,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub remote_sha: Option<String>,
    pub probed_remote_sha: Option<String>,
    pub ahead_count: Option<u32>,
    pub behind_count: Option<u32>,
    pub error_message: Option<String>,
    pub status_slow_skipped: bool,
    pub status_slow_streak: u32,
    pub stale_index_suspected: bool,
    pub last_index_refresh_at: Option<u64>,
    pub stale_locks: Vec<String>,
    pub probe_failed_reason: Option<String>,
    pub probe_issue: Option<ProbeRepoIssue>,
    pub fetched: bool,
    pub dirty_tracked: Option<bool>,
    pub dirty_tracked_at: Option<u64>,
    pub has_untracked: Option<bool>,
    pub has_untracked_at: Option<u64>,
    pub recent_commits: Vec<StoredCommit>,
    pub icon_path: Option<PathBuf>,
    pub latest_release_tag: Option<String>,
    pub latest_release_date: Option<String>,
    pub latest_release_assets: Vec<StoredReleaseAsset>,
    pub installed_version: Option<String>,
    pub last_inspected_at: Option<u64>,
    pub last_probed_at: Option<u64>,
    pub last_fetched_at: Option<u64>,
    pub fingerprint: Option<StoredFingerprint>,
}

impl Default for CachedRepoRecord {
    fn default() -> Self {
        Self {
            project_id: 0,
            key: RepoKey::default(),
            destination: DestinationKind::Missing,
            status: CloneStatus::NotCloned,
            branch: None,
            head_sha: None,
            remote_sha: None,
            probed_remote_sha: None,
            ahead_count: None,
            behind_count: None,
            error_message: None,
            status_slow_skipped: false,
            status_slow_streak: 0,
            stale_index_suspected: false,
            last_index_refresh_at: None,
            stale_locks: Vec::new(),
            probe_failed_reason: None,
            probe_issue: None,
            fetched: false,
            dirty_tracked: None,
            dirty_tracked_at: None,
            has_untracked: None,
            has_untracked_at: None,
            recent_commits: Vec::new(),
            icon_path: None,
            latest_release_tag: None,
            latest_release_date: None,
            latest_release_assets: Vec::new(),
            installed_version: None,
            last_inspected_at: None,
            last_probed_at: None,
            last_fetched_at: None,
            fingerprint: None,
        }
    }
}

impl CachedRepoRecord {
    pub fn has_probed_update(&self) -> bool {
        match (&self.remote_sha, &self.probed_remote_sha) {
            (Some(local_remote), Some(probed))
                if !local_remote.is_empty() && !probed.is_empty() =>
            {
                local_remote != probed
            }
            _ => false,
        }
    }

    pub fn has_new_release(&self) -> bool {
        match (&self.latest_release_tag, &self.installed_version) {
            (Some(latest), Some(installed)) => !latest.is_empty() && latest != installed,
            _ => false,
        }
    }

    pub fn effective_clone_status(&self) -> CloneStatus {
        if self.dirty_tracked == Some(true) || self.status == CloneStatus::Dirty {
            return CloneStatus::Dirty;
        }
        if self.has_probed_update() && self.status == CloneStatus::Current {
            return CloneStatus::Behind;
        }
        self.status
    }

    pub fn to_repository_status(&self) -> RepositoryStatus {
        let status = self.effective_clone_status();
        let state = if let Some(ref err) = self.error_message
            && status == CloneStatus::Error
        {
            Err(err.clone())
        } else {
            let msg = if !self.stale_locks.is_empty() {
                Some(format!(
                    "Repository has a stale lock ({})",
                    self.stale_locks.join(", ")
                ))
            } else if self.stale_index_suspected {
                Some("Status slow (stale index cache — refresh index to speed up)".to_string())
            } else if self.status_slow_skipped {
                Some("Status slow, skipped".to_string())
            } else if let Some(ref issue) = self.probe_issue {
                Some(issue.detail_message())
            } else if let Some(ref fail) = self.probe_failed_reason {
                Some(format!("Check failed - retry ({fail})"))
            } else if self.has_probed_update() && !self.fetched {
                Some("Update available (probe detected newer upstream commit)".to_string())
            } else {
                None
            };
            Ok(CloneState {
                status,
                branch: self.branch.clone(),
                local_revision: self.head_sha.clone(),
                remote_revision: self
                    .probed_remote_sha
                    .clone()
                    .or_else(|| self.remote_sha.clone()),
                message: msg,
            })
        };
        RepositoryStatus {
            key: self.key.clone(),
            destination: self.destination.clone(),
            state,
            fetched: self.fetched || self.last_probed_at.is_some(),
        }
    }

    pub fn to_presentation(&self) -> RepositoryPresentation {
        let latest_snapshot = self.latest_release_tag.as_ref().map(|tag| {
            release_desk_store::workspace_releases::WorkspaceRelease {
                tag_name: tag.clone(),
                description: None,
                is_draft: false,
                is_prerelease: Some(false),
                published_at: self.latest_release_date.clone(),
                assets: self
                    .latest_release_assets
                    .iter()
                    .map(|a| release_desk_store::workspace_releases::WorkspaceAsset {
                        name: a.name.clone(),
                        download_url: a.download_url.clone(),
                        size: a.size,
                    })
                    .collect(),
            }
        });
        RepositoryPresentation {
            icon_path: self.icon_path.clone(),
            version_tag: self.latest_release_tag.clone(),
            latest_snapshot,
            present: self.destination.is_git_repo(),
            recent_commits: self
                .recent_commits
                .iter()
                .map(|c| GitCommit {
                    hash: c.hash.clone(),
                    short_hash: c.short_hash.clone(),
                    author_name: c.author_name.clone(),
                    date: c.date.clone(),
                    message: c.message.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeLimitRecord {
    pub volume_dev: u64,
    pub learned_concurrency: usize,
    pub ewma_latency_ms: f64,
    pub updated_at: u64,
}

fn encode_destination(kind: &DestinationKind) -> (&'static str, Option<String>) {
    match kind {
        DestinationKind::Missing => ("missing", None),
        DestinationKind::EmptyDir => ("empty_dir", None),
        DestinationKind::NonGitDir => ("non_git_dir", None),
        DestinationKind::GitRepo => ("git_repo", None),
        DestinationKind::WrongRoot { toplevel } => {
            ("wrong_root", Some(toplevel.display().to_string()))
        }
        DestinationKind::Symlink(SymlinkState::Valid) => ("symlink_valid", None),
        DestinationKind::Symlink(SymlinkState::Broken) => ("symlink_broken", None),
        DestinationKind::Unreadable(msg) => ("unreadable", Some(msg.clone())),
    }
}

fn decode_destination(tag: &str, detail: Option<String>) -> DestinationKind {
    match tag {
        "missing" => DestinationKind::Missing,
        "empty_dir" => DestinationKind::EmptyDir,
        "non_git_dir" => DestinationKind::NonGitDir,
        "git_repo" => DestinationKind::GitRepo,
        "wrong_root" => DestinationKind::WrongRoot {
            toplevel: PathBuf::from(detail.unwrap_or_default()),
        },
        "symlink_valid" => DestinationKind::Symlink(SymlinkState::Valid),
        "symlink_broken" => DestinationKind::Symlink(SymlinkState::Broken),
        _ => DestinationKind::Unreadable(detail.unwrap_or_else(|| "Unreadable".into())),
    }
}

fn encode_clone_status(s: CloneStatus) -> &'static str {
    match s {
        CloneStatus::NotCloned => "not_cloned",
        CloneStatus::Current => "current",
        CloneStatus::Behind => "behind",
        CloneStatus::Ahead => "ahead",
        CloneStatus::Diverged => "diverged",
        CloneStatus::Dirty => "dirty",
        CloneStatus::Detached => "detached",
        CloneStatus::OtherBranch => "other_branch",
        CloneStatus::Error => "error",
    }
}

fn decode_clone_status(s: &str) -> CloneStatus {
    match s {
        "not_cloned" => CloneStatus::NotCloned,
        "current" => CloneStatus::Current,
        "behind" => CloneStatus::Behind,
        "ahead" => CloneStatus::Ahead,
        "diverged" => CloneStatus::Diverged,
        "dirty" => CloneStatus::Dirty,
        "detached" => CloneStatus::Detached,
        "other_branch" => CloneStatus::OtherBranch,
        _ => CloneStatus::Error,
    }
}

/// Persistent SQLite WAL cache living in the internal app-data directory.
pub struct RepoCache {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl std::fmt::Debug for RepoCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoCache")
            .field("path", &self.path)
            .finish()
    }
}

impl RepoCache {
    pub fn default_path() -> Result<PathBuf, String> {
        let dir = PathBuf::from(default_library_dir_string());
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Cannot create cache directory {}: {e}", dir.display()))?;
        Ok(dir.join(CACHE_FILENAME))
    }

    pub fn open_default() -> Result<Self, String> {
        let path = Self::default_path()?;
        Self::open_at(&path)
    }

    pub fn open_at(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create cache dir {}: {e}", parent.display()))?;
        }
        match Self::try_open_and_migrate(path) {
            Ok(conn) => Ok(Self {
                conn: Mutex::new(conn),
                path: path.to_path_buf(),
            }),
            Err(_) => {
                // Corruption or incompatible future schema -> quarantine/delete and rebuild atomically.
                Self::remove_db_files(path);
                let conn = Self::try_open_and_migrate(path)
                    .map_err(|e| format!("Could not rebuild repository cache: {e}"))?;
                Ok(Self {
                    conn: Mutex::new(conn),
                    path: path.to_path_buf(),
                })
            }
        }
    }

    fn remove_db_files(path: &Path) {
        let _ = std::fs::remove_file(path);
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let shm = PathBuf::from(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(wal);
        let _ = std::fs::remove_file(shm);
    }

    fn try_open_and_migrate(path: &Path) -> Result<Connection, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 3000;",
        )?;
        let quick: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        if !quick.eq_ignore_ascii_case("ok") {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
                Some(quick),
            ));
        }
        let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > CACHE_SCHEMA_VERSION {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Self::migrate(&conn, version)?;
        Ok(conn)
    }

    fn migrate(conn: &Connection, from_version: i32) -> Result<(), rusqlite::Error> {
        if from_version == 0 {
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS repo_states (
                     project_id TEXT NOT NULL,
                     provider TEXT NOT NULL,
                     host TEXT NOT NULL,
                     owner TEXT NOT NULL,
                     name TEXT NOT NULL,
                     destination_kind TEXT NOT NULL,
                     destination_detail TEXT,
                     clone_status TEXT NOT NULL,
                     branch TEXT,
                     head_sha TEXT,
                     remote_sha TEXT,
                     probed_remote_sha TEXT,
                     ahead_count INTEGER,
                     behind_count INTEGER,
                     error_message TEXT,
                     status_slow_skipped INTEGER NOT NULL DEFAULT 0,
                     status_slow_streak INTEGER NOT NULL DEFAULT 0,
                     stale_index_suspected INTEGER NOT NULL DEFAULT 0,
                     last_index_refresh_at INTEGER,
                     stale_locks_json TEXT NOT NULL DEFAULT '[]',
                     probe_failed_reason TEXT,
                     probe_issue_json TEXT,
                     fetched INTEGER NOT NULL DEFAULT 0,
                     dirty_tracked INTEGER,
                     dirty_tracked_at INTEGER,
                     has_untracked INTEGER,
                     has_untracked_at INTEGER,
                     recent_commits_json TEXT NOT NULL DEFAULT '[]',
                     icon_path TEXT,
                     latest_release_tag TEXT,
                     latest_release_date TEXT,
                     latest_release_assets_json TEXT NOT NULL DEFAULT '[]',
                     installed_version TEXT,
                     last_inspected_at INTEGER,
                     last_probed_at INTEGER,
                     last_fetched_at INTEGER,
                     fingerprint_json TEXT,
                     PRIMARY KEY (project_id, provider, host, owner, name)
                 );
                 CREATE TABLE IF NOT EXISTS volume_limits (
                     volume_dev INTEGER PRIMARY KEY,
                     learned_concurrency INTEGER NOT NULL,
                     ewma_latency_ms REAL NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
            return Ok(());
        }
        if from_version <= 1 {
            // Migrate legacy v1 -> v2
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE repo_states ADD COLUMN probed_remote_sha TEXT;
                 ALTER TABLE repo_states ADD COLUMN probe_failed_reason TEXT;
                 ALTER TABLE repo_states ADD COLUMN installed_version TEXT;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        if from_version <= 2 {
            // Migrate v2 -> v3
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE repo_states ADD COLUMN status_slow_streak INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE repo_states ADD COLUMN stale_index_suspected INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE repo_states ADD COLUMN last_index_refresh_at INTEGER;
                 ALTER TABLE repo_states ADD COLUMN stale_locks_json TEXT NOT NULL DEFAULT '[]';
                 ALTER TABLE repo_states ADD COLUMN probe_issue_json TEXT;
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        Ok(())
    }

    pub fn load_project_repos(&self, project_id: u128) -> HashMap<RepoKey, CachedRepoRecord> {
        TRACE_CACHE_READS.fetch_add(1, Ordering::Relaxed);
        let mut out = HashMap::new();
        let Ok(conn) = self.conn.lock() else {
            return out;
        };
        let pid = project_id.to_string();
        let Ok(mut stmt) = conn.prepare(
            "SELECT provider, host, owner, name, destination_kind, destination_detail,
                    clone_status, branch, head_sha, remote_sha, probed_remote_sha,
                    ahead_count, behind_count, error_message, status_slow_skipped,
                    probe_failed_reason, fetched, dirty_tracked, dirty_tracked_at,
                    has_untracked, has_untracked_at, recent_commits_json, icon_path,
                    latest_release_tag, latest_release_date, latest_release_assets_json,
                    installed_version, last_inspected_at, last_probed_at, last_fetched_at,
                    fingerprint_json, status_slow_streak, stale_index_suspected,
                    last_index_refresh_at, stale_locks_json, probe_issue_json
             FROM repo_states
             WHERE project_id = ?1",
        ) else {
            return out;
        };
        let Ok(rows) = stmt.query_map(params![pid], |row| {
            let provider: String = row.get(0)?;
            let host: String = row.get(1)?;
            let owner: String = row.get(2)?;
            let name: String = row.get(3)?;
            let dest_kind: String = row.get(4)?;
            let dest_detail: Option<String> = row.get(5)?;
            let clone_status_str: String = row.get(6)?;
            let commits_json: String = row.get(21)?;
            let icon_path: Option<String> = row.get(22)?;
            let assets_json: String = row.get(25)?;
            let fp_json: Option<String> = row.get(30)?;
            let locks_json: String = row.get(34)?;
            let issue_json: Option<String> = row.get(35)?;
            Ok(CachedRepoRecord {
                project_id,
                key: RepoKey::new(&provider, &host, &owner, &name),
                destination: decode_destination(&dest_kind, dest_detail),
                status: decode_clone_status(&clone_status_str),
                branch: row.get(7)?,
                head_sha: row.get(8)?,
                remote_sha: row.get(9)?,
                probed_remote_sha: row.get(10)?,
                ahead_count: row.get::<_, Option<i64>>(11)?.map(|v| v.max(0) as u32),
                behind_count: row.get::<_, Option<i64>>(12)?.map(|v| v.max(0) as u32),
                error_message: row.get(13)?,
                status_slow_skipped: row.get::<_, i64>(14)? != 0,
                status_slow_streak: row.get::<_, i64>(31)?.max(0) as u32,
                stale_index_suspected: row.get::<_, i64>(32)? != 0,
                last_index_refresh_at: row.get::<_, Option<i64>>(33)?.map(|v| v.max(0) as u64),
                stale_locks: serde_json::from_str(&locks_json).unwrap_or_default(),
                probe_failed_reason: row.get(15)?,
                probe_issue: issue_json.and_then(|s| serde_json::from_str(&s).ok()),
                fetched: row.get::<_, i64>(16)? != 0,
                dirty_tracked: row.get::<_, Option<i64>>(17)?.map(|v| v != 0),
                dirty_tracked_at: row.get::<_, Option<i64>>(18)?.map(|v| v.max(0) as u64),
                has_untracked: row.get::<_, Option<i64>>(19)?.map(|v| v != 0),
                has_untracked_at: row.get::<_, Option<i64>>(20)?.map(|v| v.max(0) as u64),
                recent_commits: serde_json::from_str(&commits_json).unwrap_or_default(),
                icon_path: icon_path.map(PathBuf::from),
                latest_release_tag: row.get(23)?,
                latest_release_date: row.get(24)?,
                latest_release_assets: serde_json::from_str(&assets_json).unwrap_or_default(),
                installed_version: row.get(26)?,
                last_inspected_at: row.get::<_, Option<i64>>(27)?.map(|v| v.max(0) as u64),
                last_probed_at: row.get::<_, Option<i64>>(28)?.map(|v| v.max(0) as u64),
                last_fetched_at: row.get::<_, Option<i64>>(29)?.map(|v| v.max(0) as u64),
                fingerprint: fp_json.and_then(|s| serde_json::from_str(&s).ok()),
            })
        }) else {
            return out;
        };
        for rec in rows.flatten() {
            out.insert(rec.key.clone(), rec);
        }
        out
    }

    pub fn upsert_repo(&self, rec: &CachedRepoRecord) -> Result<(), String> {
        self.upsert_repos_batch(std::slice::from_ref(rec))
    }

    pub fn upsert_repos_batch(&self, records: &[CachedRepoRecord]) -> Result<(), String> {
        if records.is_empty() {
            return Ok(());
        }
        TRACE_CACHE_WRITES.fetch_add(records.len(), Ordering::Relaxed);
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| "Cache lock poisoned".to_string())?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("Cache tx error: {e}"))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO repo_states (
                        project_id, provider, host, owner, name, destination_kind, destination_detail,
                        clone_status, branch, head_sha, remote_sha, probed_remote_sha,
                        ahead_count, behind_count, error_message, status_slow_skipped,
                        probe_failed_reason, fetched, dirty_tracked, dirty_tracked_at,
                        has_untracked, has_untracked_at, recent_commits_json, icon_path,
                        latest_release_tag, latest_release_date, latest_release_assets_json,
                        installed_version, last_inspected_at, last_probed_at, last_fetched_at,
                        fingerprint_json, status_slow_streak, stale_index_suspected,
                        last_index_refresh_at, stale_locks_json, probe_issue_json
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                        ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32,
                        ?33, ?34, ?35, ?36, ?37
                    )
                    ON CONFLICT(project_id, provider, host, owner, name) DO UPDATE SET
                        destination_kind = excluded.destination_kind,
                        destination_detail = excluded.destination_detail,
                        clone_status = excluded.clone_status,
                        branch = excluded.branch,
                        head_sha = excluded.head_sha,
                        remote_sha = excluded.remote_sha,
                        probed_remote_sha = excluded.probed_remote_sha,
                        ahead_count = excluded.ahead_count,
                        behind_count = excluded.behind_count,
                        error_message = excluded.error_message,
                        status_slow_skipped = excluded.status_slow_skipped,
                        probe_failed_reason = excluded.probe_failed_reason,
                        fetched = excluded.fetched,
                        dirty_tracked = excluded.dirty_tracked,
                        dirty_tracked_at = excluded.dirty_tracked_at,
                        has_untracked = excluded.has_untracked,
                        has_untracked_at = excluded.has_untracked_at,
                        recent_commits_json = excluded.recent_commits_json,
                        icon_path = excluded.icon_path,
                        latest_release_tag = excluded.latest_release_tag,
                        latest_release_date = excluded.latest_release_date,
                        latest_release_assets_json = excluded.latest_release_assets_json,
                        installed_version = excluded.installed_version,
                        last_inspected_at = excluded.last_inspected_at,
                        last_probed_at = excluded.last_probed_at,
                        last_fetched_at = excluded.last_fetched_at,
                        fingerprint_json = excluded.fingerprint_json,
                        status_slow_streak = excluded.status_slow_streak,
                        stale_index_suspected = excluded.stale_index_suspected,
                        last_index_refresh_at = excluded.last_index_refresh_at,
                        stale_locks_json = excluded.stale_locks_json,
                        probe_issue_json = excluded.probe_issue_json",
                )
                .map_err(|e| format!("Cache prepare error: {e}"))?;

            for rec in records {
                let (dest_tag, dest_detail) = encode_destination(&rec.destination);
                let commits_json =
                    serde_json::to_string(&rec.recent_commits).unwrap_or_else(|_| "[]".into());
                let assets_json = serde_json::to_string(&rec.latest_release_assets)
                    .unwrap_or_else(|_| "[]".into());
                let fp_json = rec
                    .fingerprint
                    .as_ref()
                    .and_then(|f| serde_json::to_string(f).ok());
                let locks_json =
                    serde_json::to_string(&rec.stale_locks).unwrap_or_else(|_| "[]".into());
                let issue_json = rec
                    .probe_issue
                    .as_ref()
                    .and_then(|i| serde_json::to_string(i).ok());
                stmt.execute(params![
                    rec.project_id.to_string(),
                    rec.key.provider,
                    rec.key.host,
                    rec.key.owner,
                    rec.key.name,
                    dest_tag,
                    dest_detail,
                    encode_clone_status(rec.status),
                    rec.branch,
                    rec.head_sha,
                    rec.remote_sha,
                    rec.probed_remote_sha,
                    rec.ahead_count.map(|v| v as i64),
                    rec.behind_count.map(|v| v as i64),
                    rec.error_message,
                    i64::from(rec.status_slow_skipped),
                    rec.probe_failed_reason,
                    i64::from(rec.fetched),
                    rec.dirty_tracked.map(i64::from),
                    rec.dirty_tracked_at.map(|v| v as i64),
                    rec.has_untracked.map(i64::from),
                    rec.has_untracked_at.map(|v| v as i64),
                    commits_json,
                    rec.icon_path.as_ref().map(|p| p.display().to_string()),
                    rec.latest_release_tag,
                    rec.latest_release_date,
                    assets_json,
                    rec.installed_version,
                    rec.last_inspected_at.map(|v| v as i64),
                    rec.last_probed_at.map(|v| v as i64),
                    rec.last_fetched_at.map(|v| v as i64),
                    fp_json,
                    i64::from(rec.status_slow_streak),
                    i64::from(rec.stale_index_suspected),
                    rec.last_index_refresh_at.map(|v| v as i64),
                    locks_json,
                    issue_json,
                ])
                .map_err(|e| format!("Cache write error: {e}"))?;
            }
        }
        tx.commit().map_err(|e| format!("Cache commit error: {e}"))
    }

    pub fn retain_project_keys(
        &self,
        project_id: u128,
        valid_keys: &HashSet<RepoKey>,
    ) -> Result<usize, String> {
        let existing = self.load_project_repos(project_id);
        let stale: Vec<RepoKey> = existing
            .keys()
            .filter(|k| !valid_keys.contains(*k))
            .cloned()
            .collect();
        if stale.is_empty() {
            return Ok(0);
        }
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| "Cache lock poisoned".to_string())?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("Cache tx error: {e}"))?;
        let pid = project_id.to_string();
        for key in &stale {
            tx.execute(
                "DELETE FROM repo_states WHERE project_id = ?1 AND provider = ?2 AND host = ?3 AND owner = ?4 AND name = ?5",
                params![pid, key.provider, key.host, key.owner, key.name],
            )
            .map_err(|e| format!("Cache delete error: {e}"))?;
        }
        tx.commit()
            .map_err(|e| format!("Cache commit error: {e}"))?;
        Ok(stale.len())
    }

    pub fn delete_repo(&self, project_id: u128, key: &RepoKey) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "Cache lock poisoned".to_string())?;
        conn.execute(
            "DELETE FROM repo_states WHERE project_id = ?1 AND provider = ?2 AND host = ?3 AND owner = ?4 AND name = ?5",
            params![
                project_id.to_string(),
                key.provider,
                key.host,
                key.owner,
                key.name,
            ],
        )
        .map_err(|e| format!("Cache delete error: {e}"))?;
        Ok(())
    }

    pub fn delete_project(&self, project_id: u128) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "Cache lock poisoned".to_string())?;
        conn.execute(
            "DELETE FROM repo_states WHERE project_id = ?1",
            params![project_id.to_string()],
        )
        .map_err(|e| format!("Cache project delete error: {e}"))?;
        Ok(())
    }

    pub fn load_volume_limit(&self, volume_dev: u64) -> Option<VolumeLimitRecord> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT learned_concurrency, ewma_latency_ms, updated_at FROM volume_limits WHERE volume_dev = ?1",
            params![volume_dev as i64],
            |row| {
                Ok(VolumeLimitRecord {
                    volume_dev,
                    learned_concurrency: row.get::<_, i64>(0)?.max(1) as usize,
                    ewma_latency_ms: row.get(1)?,
                    updated_at: row.get::<_, i64>(2)?.max(0) as u64,
                })
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn save_volume_limit(&self, rec: &VolumeLimitRecord) -> Result<(), String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "Cache lock poisoned".to_string())?;
        conn.execute(
            "INSERT INTO volume_limits (volume_dev, learned_concurrency, ewma_latency_ms, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(volume_dev) DO UPDATE SET
                 learned_concurrency = excluded.learned_concurrency,
                 ewma_latency_ms = excluded.ewma_latency_ms,
                 updated_at = excluded.updated_at",
            params![
                rec.volume_dev as i64,
                rec.learned_concurrency as i64,
                rec.ewma_latency_ms,
                rec.updated_at as i64,
            ],
        )
        .map_err(|e| format!("Volume limit save error: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(project_id: u128, name: &str) -> CachedRepoRecord {
        CachedRepoRecord {
            project_id,
            key: RepoKey::new("github", "github.com", "org", name),
            destination: DestinationKind::GitRepo,
            status: CloneStatus::Current,
            branch: Some("main".into()),
            head_sha: Some("abc1234".into()),
            remote_sha: Some("abc1234".into()),
            probed_remote_sha: Some("def5678".into()),
            ahead_count: Some(0),
            behind_count: Some(0),
            error_message: None,
            status_slow_skipped: false,
            probe_failed_reason: None,
            fetched: false,
            dirty_tracked: Some(false),
            dirty_tracked_at: Some(1_700_000_000),
            has_untracked: None,
            has_untracked_at: None,
            recent_commits: vec![],
            icon_path: None,
            latest_release_tag: Some("v2.0.0".into()),
            latest_release_date: Some("2026-11-01".into()),
            latest_release_assets: vec![],
            installed_version: Some("v1.0.0".into()),
            last_inspected_at: Some(1_700_000_000),
            last_probed_at: Some(1_700_000_050),
            last_fetched_at: None,
            fingerprint: None,
            ..Default::default()
        }
    }

    #[test]
    fn roundtrip_persist_and_stale_key_invalidation() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.sqlite3");
        {
            let cache = RepoCache::open_at(&db_path).unwrap();
            let r1 = sample_record(1, "alpha");
            let r2 = sample_record(1, "beta");
            cache.upsert_repos_batch(&[r1, r2]).unwrap();
            assert!(
                cache
                    .load_project_repos(1)
                    .get(&RepoKey::new("github", "github.com", "org", "alpha"))
                    .unwrap()
                    .has_probed_update()
            );
            assert!(
                cache
                    .load_project_repos(1)
                    .get(&RepoKey::new("github", "github.com", "org", "alpha"))
                    .unwrap()
                    .has_new_release()
            );

            let valid = HashSet::from([RepoKey::new("github", "github.com", "org", "beta")]);
            assert_eq!(cache.retain_project_keys(1, &valid).unwrap(), 1);
        }

        // Simulate app restart
        let reopened = RepoCache::open_at(&db_path).unwrap();
        let rows = reopened.load_project_repos(1);
        assert_eq!(rows.len(), 1);
        assert!(rows.contains_key(&RepoKey::new("github", "github.com", "org", "beta")));
    }

    #[test]
    fn corrupt_cache_file_is_rebuilt_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("corrupt.sqlite3");
        std::fs::write(
            &db_path,
            b"NOT_A_VALID_SQLITE_DATABASE_HEADER_AT_ALL_123456",
        )
        .unwrap();

        let cache = RepoCache::open_at(&db_path).expect("corrupt DB should rebuild cleanly");
        cache.upsert_repo(&sample_record(7, "recovered")).unwrap();
        assert_eq!(cache.load_project_repos(7).len(), 1);
    }

    #[test]
    fn schema_v1_migrates_to_v2_preserving_cached_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("legacy.sqlite3");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE repo_states (
                     project_id TEXT NOT NULL,
                     provider TEXT NOT NULL,
                     host TEXT NOT NULL,
                     owner TEXT NOT NULL,
                     name TEXT NOT NULL,
                     destination_kind TEXT NOT NULL,
                     destination_detail TEXT,
                     clone_status TEXT NOT NULL,
                     branch TEXT,
                     head_sha TEXT,
                     remote_sha TEXT,
                     ahead_count INTEGER,
                     behind_count INTEGER,
                     error_message TEXT,
                     status_slow_skipped INTEGER NOT NULL DEFAULT 0,
                     fetched INTEGER NOT NULL DEFAULT 0,
                     dirty_tracked INTEGER,
                     dirty_tracked_at INTEGER,
                     has_untracked INTEGER,
                     has_untracked_at INTEGER,
                     recent_commits_json TEXT NOT NULL DEFAULT '[]',
                     icon_path TEXT,
                     latest_release_tag TEXT,
                     latest_release_date TEXT,
                     latest_release_assets_json TEXT NOT NULL DEFAULT '[]',
                     last_inspected_at INTEGER,
                     last_probed_at INTEGER,
                     last_fetched_at INTEGER,
                     fingerprint_json TEXT,
                     PRIMARY KEY (project_id, provider, host, owner, name)
                 );
                 CREATE TABLE volume_limits (
                     volume_dev INTEGER PRIMARY KEY,
                     learned_concurrency INTEGER NOT NULL,
                     ewma_latency_ms REAL NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 INSERT INTO repo_states (project_id, provider, host, owner, name, destination_kind, clone_status)
                 VALUES ('9', 'github', 'github.com', 'org', 'legacy-repo', 'git_repo', 'current');
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }
        let migrated = RepoCache::open_at(&db_path).unwrap();
        let rows = migrated.load_project_repos(9);
        assert!(rows.contains_key(&RepoKey::new("github", "github.com", "org", "legacy-repo")));
        let conn = migrated.conn.lock().unwrap();
        let v: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, CACHE_SCHEMA_VERSION);
    }
}
