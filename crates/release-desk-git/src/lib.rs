//! System Git adapter for inspecting and updating managed repository clones.
//!
//! Every Git invocation passes separate arguments to [`Command`], never a shell.
//! Project credentials are supplied by a narrowly scoped, per-command helper.

pub mod adapter;
pub mod fingerprint;
pub mod perf;
pub use adapter::SystemGitPort;
pub use fingerprint::{FileStamp, RepoFingerprint};

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static PARTIAL_MARKER: &str = ".release-desk-partial";
static NEXT_PARTIAL_ID: AtomicU64 = AtomicU64::new(1);
pub const DEFAULT_LOCAL_GIT_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_NETWORK_GIT_TIMEOUT: Duration = Duration::from_secs(180);
pub const DEFAULT_NETWORK_STALL_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_NETWORK_MAX_TIMEOUT: Duration = Duration::from_secs(1800);
pub const DEFAULT_TERMINATION_GRACE: Duration = Duration::from_millis(3500);
pub const DEFAULT_STALE_PARTIAL_MIN_AGE: Duration = Duration::from_secs(300);

/// Detect `.git/index.lock`, `FETCH_HEAD.lock`, `shallow.lock`, and `refs/**/*.lock` without removing them.
pub fn detect_stale_locks(repo_path: &Path) -> Vec<PathBuf> {
    let Some((gitdir, commondir)) = fingerprint::resolve_git_dirs(repo_path) else {
        return Vec::new();
    };
    let mut locks = Vec::new();
    for candidate in [
        gitdir.join("index.lock"),
        gitdir.join("FETCH_HEAD.lock"),
        gitdir.join("shallow.lock"),
        commondir.join("FETCH_HEAD.lock"),
        commondir.join("shallow.lock"),
    ] {
        if candidate.exists() && !locks.contains(&candidate) {
            locks.push(candidate);
        }
    }
    fn collect_ref_locks(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                collect_ref_locks(&path, out);
            } else if path.extension().is_some_and(|e| e == "lock") && !out.contains(&path) {
                out.push(path);
            }
        }
    }
    collect_ref_locks(&commondir.join("refs"), &mut locks);
    if commondir != gitdir {
        collect_ref_locks(&gitdir.join("refs"), &mut locks);
    }
    locks.sort();
    locks
}

/// Remove detected `.git` lock files ONLY after explicit user confirmation.
pub fn remove_stale_locks_explicit(repo_path: &Path) -> Result<usize, GitError> {
    let locks = detect_stale_locks(repo_path);
    let mut removed = 0usize;
    for lock in locks {
        fs::remove_file(&lock).map_err(|source| GitError::Io {
            operation: "remove stale lock",
            source,
        })?;
        removed += 1;
    }
    Ok(removed)
}

/// Symlink classification for destination checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymlinkState {
    Valid,
    Broken,
}

/// Single source of truth classification for a repository destination path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DestinationKind {
    Missing,
    EmptyDir,
    NonGitDir,
    GitRepo,
    WrongRoot { toplevel: PathBuf },
    Symlink(SymlinkState),
    Unreadable(String),
}

impl DestinationKind {
    /// Whether this destination can be cloned into directly without deleting user data.
    pub fn is_clonable(&self) -> bool {
        matches!(self, Self::Missing | Self::EmptyDir)
    }

    /// Whether this destination looks like an incomplete or mismatched local state.
    pub fn is_incomplete(&self) -> bool {
        matches!(
            self,
            Self::EmptyDir | Self::NonGitDir | Self::WrongRoot { .. }
        )
    }

    /// Whether this destination is a valid Git repository rooted at the expected path.
    pub fn is_git_repo(&self) -> bool {
        matches!(self, Self::GitRepo)
    }
}

/// Live progress parsed from `git clone --progress`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneProgress {
    pub percent: Option<u8>,
    pub detail: String,
}

/// Parse a single progress line/CR chunk emitted by `git clone --progress`.
pub fn parse_git_progress(chunk: &str) -> Option<CloneProgress> {
    let trimmed = chunk.trim();
    if trimmed.is_empty() {
        return None;
    }
    let pct_pos = trimmed.find('%')?;
    let prefix = &trimmed[..pct_pos];
    let digits_start = prefix
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |idx| idx + 1);
    let pct: u8 = prefix[digits_start..].parse().ok()?;
    Some(CloneProgress {
        percent: Some(pct.min(100)),
        detail: trimmed.to_string(),
    })
}

/// Pure-stat fast precheck verifying `path` is under `project_root`, has no symlink traversal,
/// is a real directory, and owns a direct `.git` directory or `.git` file (worktree/submodule).
pub fn path_looks_like_direct_git_repo(project_root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(project_root) else {
        return false;
    };
    let mut current = project_root.to_path_buf();
    for part in relative.components() {
        let Component::Normal(seg) = part else {
            return false;
        };
        current.push(seg);
        let Ok(meta) = fs::symlink_metadata(&current) else {
            return false;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return false;
        }
    }
    fs::symlink_metadata(path.join(".git")).is_ok()
}

/// Classify a repository path relative to `project_root` (checking traversal & ancestor symlinks).
pub fn classify_destination(project_root: &Path, path: &Path) -> DestinationKind {
    let Ok(relative) = path.strip_prefix(project_root) else {
        return DestinationKind::Unreadable("Repository path is outside the project folder".into());
    };
    let mut current = project_root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (idx, part) in components.iter().enumerate() {
        let Component::Normal(seg) = part else {
            return DestinationKind::Unreadable("Invalid repository path".into());
        };
        current.push(seg);
        let is_last = idx + 1 == components.len();
        if !is_last {
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return DestinationKind::Unreadable(format!(
                        "Repository path contains a symbolic link: {}",
                        current.display()
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return DestinationKind::Unreadable(format!(
                        "Cannot inspect repository path {}: {error}",
                        current.display()
                    ));
                }
            }
        }
    }
    classify_path(path)
}

/// Classify a destination path without a project root prefix check.
pub fn classify_path(path: &Path) -> DestinationKind {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return DestinationKind::Missing,
        Err(error) => return DestinationKind::Unreadable(error.to_string()),
    };
    if meta.file_type().is_symlink() {
        return if path.exists() {
            DestinationKind::Symlink(SymlinkState::Valid)
        } else {
            DestinationKind::Symlink(SymlinkState::Broken)
        };
    }
    if !meta.is_dir() {
        return DestinationKind::NonGitDir;
    }
    match fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                return DestinationKind::EmptyDir;
            }
        }
        Err(error) => return DestinationKind::Unreadable(error.to_string()),
    }

    if let Some((gitdir, commondir)) = fingerprint::resolve_git_dirs(path)
        && gitdir.join("HEAD").is_file()
        && (gitdir.join("objects").is_dir()
            || gitdir.join("reftable").is_dir()
            || commondir.join("objects").is_dir()
            || commondir.join("reftable").is_dir())
    {
        return DestinationKind::GitRepo;
    }

    let client = GitClient::new();
    let Ok(out) = client.run(
        Some(path),
        "inspect repository root",
        ["rev-parse", "--is-inside-work-tree", "--show-toplevel"],
    ) else {
        return DestinationKind::NonGitDir;
    };
    if !out.status.success() {
        return DestinationKind::NonGitDir;
    }
    let text = stdout(&out);
    let mut lines = text.lines();
    let Some(inside) = lines.next().map(str::trim) else {
        return DestinationKind::NonGitDir;
    };
    let Some(toplevel_str) = lines.next().map(str::trim) else {
        return DestinationKind::NonGitDir;
    };
    if inside != "true" {
        return DestinationKind::NonGitDir;
    }
    if paths_match(toplevel_str, path) {
        DestinationKind::GitRepo
    } else {
        DestinationKind::WrongRoot {
            toplevel: PathBuf::from(toplevel_str),
        }
    }
}

/// True when `path` is a directory whose only direct child entry is `.git`.
pub fn is_dot_git_only_dir(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    let mut saw_dot_git = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == ".git" && entry.file_type().is_ok_and(|t| t.is_dir()) {
            saw_dot_git = true;
        } else {
            return false;
        }
    }
    saw_dot_git
}

/// True when `path` carries our partial-clone marker file.
pub fn has_partial_marker(path: &Path) -> bool {
    path.join(PARTIAL_MARKER).is_file()
}

/// Return up to `limit` human-readable entry labels for confirmation dialogs, plus total count.
pub fn list_directory_preview(path: &Path, limit: usize) -> (Vec<String>, usize) {
    let Ok(entries) = fs::read_dir(path) else {
        return (Vec::new(), 0);
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| {
            let mut label = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                label.push('/');
            }
            label
        })
        .collect();
    names.sort();
    let total = names.len();
    names.truncate(limit);
    (names, total)
}

/// Redact any `scheme://user:pass@host` credentials from a URL string for safe logs/traces.
pub fn redact_url_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme_pos) = rest.find("://") {
        let split = scheme_pos + 3;
        out.push_str(&rest[..split]);
        rest = &rest[split..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | ')' | '>'))
            .unwrap_or(rest.len());
        let candidate = &rest[..end];
        if let Some(at_pos) = candidate.find('@') {
            let authority_prefix = &candidate[..at_pos];
            if !authority_prefix.contains('/') {
                out.push_str("<redacted>@");
                out.push_str(&candidate[at_pos + 1..]);
                rest = &rest[end..];
                continue;
            }
        }
        out.push_str(candidate);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

pub fn format_partial_marker(pid: u32, created_secs: u64) -> String {
    format!("pid={pid}\ncreated_secs={created_secs}\n")
}

fn parse_partial_marker(content: &str) -> Option<(u32, u64)> {
    let mut pid = None;
    let mut created_secs = None;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("pid=") {
            pid = v.trim().parse::<u32>().ok();
        } else if let Some(v) = line.strip_prefix("created_secs=") {
            created_secs = v.trim().parse::<u64>().ok();
        }
    }
    Some((pid?, created_secs?))
}

/// Check whether `pid` is still alive on Unix (`kill -0`).
#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let Ok(out) = Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
    else {
        return true;
    };
    if out.status.success() {
        return true;
    }
    // If another user's process returned EPERM, treat it as alive rather than deleting.
    let err = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
    err.contains("operation not permitted") || err.contains("not permitted")
}

#[cfg(not(unix))]
pub fn is_pid_alive(_pid: u32) -> bool {
    false
}

/// Remove stale `*.partial*` directories created by interrupted atomic clones on startup.
/// Requires ALL of: our `.release-desk-partial` marker, a dead PID, and `DEFAULT_STALE_PARTIAL_MIN_AGE`.
pub fn cleanup_stale_partial_clones(project_dirs: &[PathBuf]) -> usize {
    cleanup_stale_partial_clones_with_policy(
        project_dirs,
        DEFAULT_STALE_PARTIAL_MIN_AGE,
        is_pid_alive,
    )
}

/// Policy-parameterized stale `.partial` cleanup used for deterministic testing.
pub fn cleanup_stale_partial_clones_with_policy<F>(
    project_dirs: &[PathBuf],
    min_age: Duration,
    pid_alive: F,
) -> usize
where
    F: Fn(u32) -> bool,
{
    fn scan_dir<F: Fn(u32) -> bool>(
        dir: &Path,
        depth: usize,
        min_age: Duration,
        pid_alive: &F,
        removed: &mut usize,
    ) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() || !meta.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let marker = path.join(PARTIAL_MARKER);
            if name.contains(".partial") && marker.is_file() {
                let Ok(content) = fs::read_to_string(&marker) else {
                    continue;
                };
                let Some((pid, created_secs)) = parse_partial_marker(&content) else {
                    continue;
                };
                if pid_alive(pid) {
                    continue;
                }
                let created_time = UNIX_EPOCH + Duration::from_secs(created_secs);
                let age = now.duration_since(created_time).unwrap_or(Duration::ZERO);
                if age >= min_age && fs::remove_dir_all(&path).is_ok() {
                    *removed += 1;
                }
            } else if depth > 0 {
                scan_dir(&path, depth - 1, min_age, pid_alive, removed);
            }
        }
    }

    let mut removed = 0;
    for dir in project_dirs {
        scan_dir(dir, 1, min_age, &pid_alive, &mut removed);
    }
    removed
}

struct PartialCloneGuard {
    dir: PathBuf,
    armed: bool,
}

impl PartialCloneGuard {
    fn new(dir: PathBuf) -> Self {
        Self { dir, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PartialCloneGuard {
    fn drop(&mut self) {
        if self.armed && self.dir.join(PARTIAL_MARKER).is_file() {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

/// The observed state of a managed clone relative to its tracked default branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloneStatus {
    NotCloned,
    Current,
    Behind,
    Ahead,
    Diverged,
    Dirty,
    Detached,
    OtherBranch,
    Error,
}

/// A Git commit from the log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub author_name: String,
    pub date: String,
    pub message: String,
}

/// A snapshot of local and locally fetched remote revisions.
///
/// `remote_revision` is the last fetched `origin/<default_branch>` revision;
/// call [`GitClient::fetch`] before inspection when a fresh network view is needed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneState {
    pub status: CloneStatus,
    pub branch: Option<String>,
    pub local_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub message: Option<String>,
}

impl CloneState {
    fn new(status: CloneStatus) -> Self {
        Self {
            status,
            branch: None,
            local_revision: None,
            remote_revision: None,
            message: None,
        }
    }
}

/// Outcome of a guarded clone or fast-forward operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncOutcome {
    Cloned { revision: String },
    FastForwarded { from: String, to: String },
    AlreadyCurrent,
    Skipped(CloneState),
}

/// Git process and input failures. Command arguments and remote URLs are never stored here.
#[derive(Debug)]
pub enum GitError {
    Cancelled,
    TimedOut {
        operation: &'static str,
        timeout: Duration,
    },
    InvalidInput(&'static str),
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Command {
        operation: &'static str,
        code: Option<i32>,
        detail: Option<String>,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Git operation cancelled"),
            Self::TimedOut { operation, timeout } => {
                write!(
                    f,
                    "Git {operation} timed out after {:.1}s",
                    timeout.as_secs_f64()
                )
            }
            Self::InvalidInput(message) => write!(f, "invalid Git input: {message}"),
            Self::Io { operation, source } => write!(f, "Git {operation} failed: {source}"),
            Self::Command {
                operation,
                code,
                detail,
            } => {
                if let Some(detail) = detail.as_deref().filter(|d| !d.is_empty()) {
                    write!(f, "Git {operation} failed: {detail}")
                } else {
                    write!(f, "Git {operation} exited unsuccessfully ({code:?})")
                }
            }
        }
    }
}

impl Error for GitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Non-secret metadata used by the per-command credential helper.
#[derive(Clone, Debug)]
pub struct GitCredentialContext {
    helper_executable: std::path::PathBuf,
    credential_ref: String,
    host: String,
    path: String,
    username: String,
    remote_url: String,
}

impl GitCredentialContext {
    /// Scope a Keychain reference to one HTTPS GitHub or GitLab repository.
    pub fn new(
        helper_executable: std::path::PathBuf,
        credential_ref: String,
        remote_url: &str,
    ) -> Result<Self, GitError> {
        if !helper_executable.is_absolute()
            || helper_executable.to_str().is_none()
            || credential_ref.is_empty()
            || credential_ref.chars().any(char::is_control)
        {
            return Err(GitError::InvalidInput(
                "invalid credential helper configuration",
            ));
        }
        let expected_username = if remote_url.starts_with("https://github.com/") {
            "x-access-token"
        } else if remote_url.starts_with("https://gitlab.com/") {
            "oauth2"
        } else {
            return Err(GitError::InvalidInput(
                "project credentials are not supported for this host",
            ));
        };
        let host = remote_url
            .strip_prefix("https://")
            .and_then(|rest| rest.split_once('/'))
            .map(|(host, _)| host)
            .ok_or(GitError::InvalidInput("remote has no repository path"))?;
        Self::for_provider(
            helper_executable,
            credential_ref,
            remote_url,
            host,
            expected_username,
        )
    }

    /// Scope a selected provider credential to an exact HTTPS remote and host.
    pub fn for_provider(
        helper_executable: std::path::PathBuf,
        credential_ref: String,
        remote_url: &str,
        expected_host: &str,
        username: &str,
    ) -> Result<Self, GitError> {
        if !helper_executable.is_absolute()
            || helper_executable.to_str().is_none()
            || credential_ref.is_empty()
            || credential_ref.chars().any(char::is_control)
            || username.is_empty()
            || username.chars().any(char::is_control)
            || expected_host.is_empty()
            || !expected_host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b':'))
        {
            return Err(GitError::InvalidInput(
                "invalid credential helper configuration",
            ));
        }
        let rest = remote_url
            .strip_prefix("https://")
            .ok_or(GitError::InvalidInput(
                "project credentials require an HTTPS remote",
            ))?;
        let (host, path) = rest
            .split_once('/')
            .ok_or(GitError::InvalidInput("remote has no repository path"))?;
        if host != expected_host {
            return Err(GitError::InvalidInput(
                "remote host does not match the provider",
            ));
        }
        if path.is_empty()
            || path.starts_with('/')
            || path
                .chars()
                .any(|c| c.is_control() || matches!(c, '?' | '#' | '\\' | '@'))
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(GitError::InvalidInput("invalid HTTPS repository path"));
        }
        Ok(Self {
            helper_executable,
            credential_ref,
            host: host.into(),
            path: path.into(),
            username: username.into(),
            remote_url: remote_url.into(),
        })
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Runs system Git without a shell or command-line credentials.
#[derive(Clone, Debug)]
pub struct GitClient {
    executable: std::ffi::OsString,
    credential: Option<GitCredentialContext>,
    token: Option<String>,
    codecommit_credentials: bool,
    credential_manager: bool,
    timeout: Option<Duration>,
    stall_timeout: Option<Duration>,
    termination_grace: Duration,
    low_io_priority: bool,
}

impl Default for GitClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitClient {
    pub fn new() -> Self {
        Self {
            executable: "git".into(),
            credential: None,
            token: None,
            codecommit_credentials: false,
            credential_manager: false,
            timeout: None,
            stall_timeout: None,
            termination_grace: DEFAULT_TERMINATION_GRACE,
            low_io_priority: false,
        }
    }

    /// Override the per-subprocess execution timeout (falls back to operation defaults when unset).
    pub fn with_executable(mut self, executable: impl Into<std::ffi::OsString>) -> Self {
        self.executable = executable.into();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Override the network inactivity stall timeout (when no stdout/stderr bytes arrive).
    pub fn with_stall_timeout(mut self, timeout: Duration) -> Self {
        self.stall_timeout = Some(timeout);
        self
    }

    /// Override the SIGTERM-to-SIGKILL grace period used on cancellation or timeout.
    pub fn with_termination_grace(mut self, grace: Duration) -> Self {
        self.termination_grace = grace;
        self
    }

    /// Run spawned git subprocesses at background/throttled I/O priority when supported by the OS.
    pub fn with_low_io_priority(mut self, enabled: bool) -> Self {
        self.low_io_priority = enabled;
        self
    }

    /// Use a selected credential for this exact remote; no token is stored here.
    pub fn with_credential(mut self, credential: GitCredentialContext) -> Self {
        self.credential = Some(credential);
        self
    }

    /// Supply an authentication token used for HTTPS Basic Authorization header (e.g. from `gh auth token`).
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Use the AWS CLI's CodeCommit Git credential helper for an AWS remote.
    pub fn with_codecommit_credentials(mut self) -> Self {
        self.codecommit_credentials = true;
        self
    }

    /// Ask Git Credential Manager for a fresh credential on each Git operation.
    pub fn with_credential_manager(mut self) -> Self {
        self.credential_manager = true;
        self
    }

    fn validate_credential_remote(&self, remote_url: &str) -> Result<(), GitError> {
        if self
            .credential
            .as_ref()
            .is_some_and(|credential| credential.remote_url != remote_url)
        {
            return Err(GitError::InvalidInput(
                "credential is scoped to a different remote",
            ));
        }
        Ok(())
    }

    /// Inspect a clone without fetching or changing its working tree.
    pub fn inspect(&self, path: &Path, default_branch: &str) -> Result<CloneState, GitError> {
        let total_t = perf::start();
        let sub_before = perf::reset_subprocess_counter();
        let mut timings = perf::InspectTimings::default();
        let outcome = self.inspect_porcelain_v2_inner(path, default_branch, true, true);
        if total_t.is_some() {
            timings.total_ns = perf::elapsed_ns(total_t);
            let (subs, sub_ns) = perf::subprocess_delta(sub_before);
            timings.subprocesses = subs;
            timings.subprocess_ns = sub_ns;
            perf::store_inspect(timings);
        }
        outcome.map(|(state, _, _, _)| state)
    }

    /// Inspect ref status (`HEAD`, branch, `origin/<default_branch>`, ahead/behind) AND tracked
    /// dirty state in a SINGLE `git --no-optional-locks status --porcelain=v2 --branch -uno`
    /// subprocess whenever `branch.upstream` matches `origin/<default_branch>` and commits match.
    /// Falls back cleanly to `rev-parse` + `rev-list` when upstream differs or is unset.
    pub fn inspect_refs_and_dirty_uno(
        &self,
        path: &Path,
        default_branch: &str,
    ) -> Result<(CloneState, Option<u32>, Option<u32>, bool), GitError> {
        self.inspect_porcelain_v2_inner(path, default_branch, false, false)
    }

    fn inspect_porcelain_v2_inner(
        &self,
        path: &Path,
        default_branch: &str,
        include_untracked: bool,
        dirty_sets_status: bool,
    ) -> Result<(CloneState, Option<u32>, Option<u32>, bool), GitError> {
        validate_branch(default_branch)?;
        let kind = classify_path(path);
        if let Some(early) = destination_error_state(kind) {
            return Ok((early, None, None, false));
        }

        let untracked_flag = if include_untracked {
            "--untracked-files=normal"
        } else {
            "-uno"
        };
        let status_out = self.run(
            Some(path),
            "inspect changes",
            [
                "--no-optional-locks",
                "status",
                "--porcelain=v2",
                "--branch",
                untracked_flag,
            ],
        )?;
        if !status_out.status.success() {
            return Err(command_error("inspect changes", &status_out));
        }
        let summary = PorcelainV2Summary::parse(&String::from_utf8_lossy(&status_out.stdout));
        let mut state = CloneState::new(CloneStatus::Error);
        state.local_revision = summary.head_sha.clone();
        state.branch = summary.branch.clone();
        let is_dirty = if include_untracked {
            summary.dirty_any
        } else {
            summary.dirty_tracked
        };

        let expected_upstream = format!("origin/{default_branch}");
        if !summary.detached
            && (!dirty_sets_status || !is_dirty)
            && state.branch.as_deref() == Some(default_branch)
            && summary.upstream.as_deref() == Some(expected_upstream.as_str())
            && let Some((ahead, behind)) = summary.ab
            && ahead == 0
            && behind == 0
            && state.local_revision.is_some()
        {
            // Fast path: 1 subprocess total (local == origin/<default_branch>).
            state.remote_revision = state.local_revision.clone();
            state.status = CloneStatus::Current;
            return Ok((state, Some(0), Some(0), is_dirty));
        }

        let remote_ref = format!("refs/remotes/origin/{default_branch}");
        let remote = self.run(
            Some(path),
            "read remote revision",
            ["rev-parse", "--verify", remote_ref.as_str()],
        )?;
        if remote.status.success() {
            state.remote_revision = Some(stdout(&remote));
        }

        let counts = match (&state.local_revision, &state.remote_revision) {
            (Some(local), Some(remote_sha)) => {
                if !summary.detached
                    && state.branch.as_deref() == Some(default_branch)
                    && summary.upstream.as_deref() == Some(expected_upstream.as_str())
                    && let Some(ab) = summary.ab
                {
                    Some(ab)
                } else if local == remote_sha {
                    Some((0, 0))
                } else {
                    Some(self.ahead_behind_counts(path, local, remote_sha)?)
                }
            }
            _ => None,
        };
        let ahead_opt = counts.map(|(a, _)| a);
        let behind_opt = counts.map(|(_, b)| b);

        if summary.detached {
            state.status = CloneStatus::Detached;
            return Ok((state, ahead_opt, behind_opt, is_dirty));
        }
        if dirty_sets_status && is_dirty {
            state.status = CloneStatus::Dirty;
            let orig_head_present = fingerprint::resolve_git_dirs(path)
                .is_some_and(|(gitdir, _)| gitdir.join("ORIG_HEAD").exists());
            state.message = Some(summary.dirty_detail_message(orig_head_present));
            return Ok((state, ahead_opt, behind_opt, true));
        }
        if state.branch.as_deref() != Some(default_branch) {
            state.status = CloneStatus::OtherBranch;
            return Ok((state, ahead_opt, behind_opt, is_dirty));
        }

        let Some((ahead, behind)) = counts else {
            if state.local_revision.is_none() && state.remote_revision.is_none() {
                state.status = CloneStatus::Current;
                state.message = Some("This local copy has no commits. Check for updates to see whether the online repository has a first commit.".into());
            } else if state.local_revision.is_none() {
                state.message = Some("The local copy has no commits, but the online repository does. Use Fix local copy to restore it.".into());
            } else {
                state.message = Some(format!(
                    "The online branch {default_branch} is unavailable. Refresh this source or check its default branch."
                ));
            }
            return Ok((state, None, None, is_dirty));
        };

        state.status = match (ahead > 0, behind > 0) {
            (false, false) => CloneStatus::Current,
            (false, true) => CloneStatus::Behind,
            (true, false) => CloneStatus::Ahead,
            (true, true) => CloneStatus::Diverged,
        };
        Ok((state, Some(ahead), Some(behind), is_dirty))
    }

    /// Inspect only ref state (`HEAD`, default branch, `origin/<branch>`, and `ahead`/`behind`
    /// commit counts via 1 `rev-list --left-right --count`) without running `git status`.
    pub fn inspect_refs_only(
        &self,
        path: &Path,
        default_branch: &str,
    ) -> Result<(CloneState, Option<u32>, Option<u32>), GitError> {
        validate_branch(default_branch)?;
        let kind = classify_path(path);
        let mut state = CloneState::new(CloneStatus::Error);
        match kind {
            DestinationKind::Missing => {
                return Ok((CloneState::new(CloneStatus::NotCloned), None, None));
            }
            DestinationKind::EmptyDir => {
                state.message = Some("Empty directory at repository destination".into());
                return Ok((state, None, None));
            }
            DestinationKind::NonGitDir => {
                state.message = Some("Path is not a Git working tree".into());
                return Ok((state, None, None));
            }
            DestinationKind::WrongRoot { .. } => {
                state.message = Some("Path is inside another Git working tree".into());
                return Ok((state, None, None));
            }
            DestinationKind::Symlink(_) => {
                state.message = Some("Repository path is a symbolic link".into());
                return Ok((state, None, None));
            }
            DestinationKind::Unreadable(error) => {
                state.message = Some(error);
                return Ok((state, None, None));
            }
            DestinationKind::GitRepo => {}
        }

        let head = self.run(Some(path), "read HEAD", ["rev-parse", "--verify", "HEAD"])?;
        if head.status.success() {
            state.local_revision = Some(stdout(&head));
        }
        let remote_ref = format!("refs/remotes/origin/{default_branch}");
        let remote = self.run(
            Some(path),
            "read remote revision",
            ["rev-parse", "--verify", remote_ref.as_str()],
        )?;
        if remote.status.success() {
            state.remote_revision = Some(stdout(&remote));
        }
        let branch = self.run(
            Some(path),
            "read branch",
            ["symbolic-ref", "--quiet", "--short", "HEAD"],
        )?;
        if !branch.status.success() {
            state.status = CloneStatus::Detached;
            return Ok((state, None, None));
        }
        state.branch = Some(stdout(&branch));
        if state.branch.as_deref() != Some(default_branch) {
            state.status = CloneStatus::OtherBranch;
            return Ok((state, None, None));
        }
        let (Some(local), Some(remote_sha)) = (&state.local_revision, &state.remote_revision)
        else {
            if state.local_revision.is_none() && state.remote_revision.is_none() {
                state.status = CloneStatus::Current;
                state.message = Some("This local copy has no commits.".into());
            } else {
                state.message = Some(format!(
                    "The online branch {default_branch} is unavailable locally."
                ));
            }
            return Ok((state, None, None));
        };
        if local == remote_sha {
            state.status = CloneStatus::Current;
            return Ok((state, Some(0), Some(0)));
        }
        let (ahead, behind) = self.ahead_behind_counts(path, local, remote_sha)?;
        state.status = match (ahead > 0, behind > 0) {
            (false, false) => CloneStatus::Current,
            (false, true) => CloneStatus::Behind,
            (true, false) => CloneStatus::Ahead,
            (true, true) => CloneStatus::Diverged,
        };
        Ok((state, Some(ahead), Some(behind)))
    }

    /// Compute `(ahead, behind)` commit counts between `local` and `remote` in ONE subprocess.
    pub fn ahead_behind_counts(
        &self,
        path: &Path,
        local: &str,
        remote: &str,
    ) -> Result<(u32, u32), GitError> {
        let range = format!("{local}...{remote}");
        let out = self.run(
            Some(path),
            "compare revisions",
            ["rev-list", "--left-right", "--count", range.as_str()],
        )?;
        if !out.status.success() {
            return Err(command_error("compare revisions", &out));
        }
        let text = stdout(&out);
        let mut parts = text.split_whitespace();
        let ahead = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let behind = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        Ok((ahead, behind))
    }

    /// Cheap lock-free tracked dirty check (`git --no-optional-locks status --porcelain=v1 -uno`).
    pub fn inspect_tracked_dirty(&self, path: &Path) -> Result<bool, GitError> {
        let out = self.run(
            Some(path),
            "inspect changes",
            ["--no-optional-locks", "status", "--porcelain=v1", "-uno"],
        )?;
        if !out.status.success() {
            return Err(command_error("inspect changes", &out));
        }
        Ok(!out.stdout.is_empty())
    }

    /// Full untracked-file check using `--no-optional-locks` (for selected repo / on demand).
    pub fn inspect_untracked_present(&self, path: &Path) -> Result<bool, GitError> {
        let out = self.run(
            Some(path),
            "inspect changes",
            [
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=normal",
            ],
        )?;
        if !out.status.success() {
            return Err(command_error("inspect changes", &out));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l.starts_with("??")))
    }

    /// Lightweight network probe (`git ls-remote`) returning the remote branch head SHA
    /// without downloading any objects or touching the local repo.
    pub fn probe_remote_branch_sha(
        &self,
        remote_url: &str,
        default_branch: &str,
    ) -> Result<Option<String>, GitError> {
        validate_branch(default_branch)?;
        self.validate_credential_remote(remote_url)?;
        let ref_spec = format!("refs/heads/{default_branch}");
        let out = self.run(
            None,
            "query remote branch",
            ["ls-remote", remote_url, ref_spec.as_str()],
        )?;
        if !out.status.success() {
            return Err(command_error("query remote branch", &out));
        }
        let sha = stdout(&out)
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .filter(|s| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit()))
            .map(ToString::to_string);
        Ok(sha)
    }

    /// Fetch `origin` while leaving the working tree unchanged.
    pub fn fetch(&self, path: &Path) -> Result<(), GitError> {
        if self.credential.is_some() {
            return Err(GitError::InvalidInput(
                "credentialed fetch requires remote verification",
            ));
        }
        self.fetch_origin(path)
    }

    fn fetch_origin(&self, path: &Path) -> Result<(), GitError> {
        let output = self.run(
            Some(path),
            "fetch",
            ["fetch", "--prune", "--progress", "origin"],
        )?;
        success("fetch", output).map(|_| ())
    }

    /// Refresh cached stat info in `.git/index` at low I/O priority ONLY if no `index.lock` exists.
    /// Returns `Ok(true)` if refreshed, `Ok(false)` if locked by another process.
    pub fn refresh_index_if_unlocked(&self, path: &Path) -> Result<bool, GitError> {
        let Some((gitdir, _)) = fingerprint::resolve_git_dirs(path) else {
            return Ok(false);
        };
        if gitdir.join("index.lock").exists() {
            return Ok(false);
        }
        let client = self.clone().with_low_io_priority(true);
        let output = client.run(
            Some(path),
            "refresh index",
            ["update-index", "-q", "--refresh"],
        )?;
        // exit code 1 means files differ (modified), which still refreshes stat metadata cleanly.
        if output.status.success() || output.status.code() == Some(1) {
            Ok(true)
        } else {
            Err(command_error("refresh index", &output))
        }
    }

    /// Fetch only after verifying that this directory is the expected checkout.
    /// Runs zero pre-fetch `inspect()` subprocesses and 1 post-fetch porcelain-v2 status check
    /// (plus `remote get-url origin` + `fetch`), avoiding redundant untracked worktree scans.
    pub fn fetch_verified(
        &self,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
    ) -> Result<CloneState, GitError> {
        let total_t = perf::start();
        let sub_before = perf::reset_subprocess_counter();
        let mut timings = perf::FetchTimings::default();
        validate_branch(default_branch)?;
        self.validate_credential_remote(remote_url)?;
        if let Some(early) = destination_error_state(classify_path(path)) {
            if total_t.is_some() {
                timings.total_ns = perf::elapsed_ns(total_t);
                let (subs, sub_ns) = perf::subprocess_delta(sub_before);
                timings.subprocesses = subs;
                timings.subprocess_ns = sub_ns;
                perf::store_fetch(timings);
            }
            return Ok(early);
        }
        let t = perf::start();
        let origin = self.run(Some(path), "read origin", ["remote", "get-url", "origin"])?;
        timings.origin_verify_ns = perf::elapsed_ns(t);
        if !origin.status.success() || stdout(&origin) != remote_url {
            let mut rejected = CloneState::new(CloneStatus::Error);
            rejected.message = Some("Clone origin does not match the expected remote".into());
            if total_t.is_some() {
                timings.total_ns = perf::elapsed_ns(total_t);
                let (subs, sub_ns) = perf::subprocess_delta(sub_before);
                timings.subprocesses = subs;
                timings.subprocess_ns = sub_ns;
                perf::store_fetch(timings);
            }
            return Ok(rejected);
        }
        let t = perf::start();
        self.fetch_origin(path)?;
        timings.fetch_origin_ns = perf::elapsed_ns(t);
        let t = perf::start();
        let result = self
            .inspect_porcelain_v2_inner(path, default_branch, false, true)
            .map(|(state, _, _, _)| state);
        timings.post_inspect_ns = perf::elapsed_ns(t);
        if total_t.is_some() {
            timings.total_ns = perf::elapsed_ns(total_t);
            let (subs, sub_ns) = perf::subprocess_delta(sub_before);
            timings.subprocesses = subs;
            timings.subprocess_ns = sub_ns;
            perf::store_fetch(timings);
        }
        result
    }

    /// Clone when absent, or fetch and fast-forward a clean default-branch checkout.
    ///
    /// Dirty, detached, alternate-branch, ahead, and diverged checkouts are left untouched.
    /// The caller must supply the same default branch for a repository on each call.
    pub fn sync(
        &self,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
    ) -> Result<SyncOutcome, GitError> {
        self.sync_inner(remote_url, path, default_branch, true)
    }

    /// Clone into a newly reserved sibling `.partial` directory, verify, and atomically rename.
    ///
    /// Missing and empty directories are clonable; valid existing checkouts, symlinks, and
    /// non-git directories are refused.
    pub fn clone_only(
        &self,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
    ) -> Result<SyncOutcome, GitError> {
        let project_root = path
            .parent()
            .ok_or(GitError::InvalidInput("clone path has no parent"))?;
        self.clone_atomic_cancellable(
            project_root,
            remote_url,
            path,
            default_branch,
            false,
            &AtomicBool::new(false),
            |_| {},
        )
    }

    /// Atomically clone into `<name>.partial-<id>/checkout`, stream `--progress`, verify,
    /// and rename into `path`. Always cleans up `.partial` on failure, cancel, or panic.
    #[allow(clippy::too_many_arguments)]
    pub fn clone_atomic_cancellable(
        &self,
        project_root: &Path,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
        replace_empty_git_repo: bool,
        cancelled: &AtomicBool,
        mut on_progress: impl FnMut(CloneProgress),
    ) -> Result<SyncOutcome, GitError> {
        validate_branch(default_branch)?;
        self.validate_credential_remote(remote_url)?;
        if remote_url.is_empty()
            || remote_url.starts_with('-')
            || remote_url.chars().any(char::is_control)
            || has_embedded_credentials(remote_url)
        {
            return Err(GitError::InvalidInput("remote URL is empty or unsafe"));
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(GitError::Cancelled);
        }

        let initial_kind = classify_destination(project_root, path);
        let allow_replace_git_only = replace_empty_git_repo
            && initial_kind == DestinationKind::GitRepo
            && self
                .inspect(path, default_branch)
                .is_ok_and(|s| s.local_revision.is_none() && s.remote_revision.is_none());
        if !initial_kind.is_clonable() && !allow_replace_git_only {
            return Err(match initial_kind {
                DestinationKind::GitRepo => {
                    GitError::InvalidInput("repository destination already exists")
                }
                DestinationKind::NonGitDir => {
                    GitError::InvalidInput("non-git folder exists at destination")
                }
                DestinationKind::WrongRoot { .. } => {
                    GitError::InvalidInput("destination is inside another git repository")
                }
                DestinationKind::Symlink(_) => {
                    GitError::InvalidInput("destination is a symbolic link")
                }
                DestinationKind::Unreadable(_) => {
                    GitError::InvalidInput("destination path is unreadable or unsafe")
                }
                DestinationKind::Missing | DestinationKind::EmptyDir => unreachable!(),
            });
        }

        let parent = path
            .parent()
            .ok_or(GitError::InvalidInput("clone path has no parent"))?;
        fs::create_dir_all(parent).map_err(|source| GitError::Io {
            operation: "create clone parent",
            source,
        })?;
        let repo_file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(GitError::InvalidInput("invalid repository directory name"))?;
        let partial_id = NEXT_PARTIAL_ID.fetch_add(1, Ordering::Relaxed);
        let partial_dir = parent.join(format!(
            "{repo_file_name}.partial-{}-{partial_id}",
            std::process::id()
        ));
        fs::create_dir(&partial_dir).map_err(|source| GitError::Io {
            operation: "create partial clone directory",
            source,
        })?;
        let mut guard = PartialCloneGuard::new(partial_dir.clone());
        let created_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let marker_body = format_partial_marker(std::process::id(), created_secs);
        fs::write(partial_dir.join(PARTIAL_MARKER), marker_body).map_err(|source| {
            GitError::Io {
                operation: "write partial clone marker",
                source,
            }
        })?;
        let checkout_dir = partial_dir.join("checkout");

        let mut result = self.run_clone_with_progress(
            [
                OsStr::new("clone"),
                OsStr::new("--progress"),
                OsStr::new("--branch"),
                OsStr::new(default_branch),
                OsStr::new("--"),
                OsStr::new(remote_url),
                checkout_dir.as_os_str(),
            ],
            cancelled,
            &mut on_progress,
        );
        if matches!(&result, Err(GitError::Command { .. })) && !cancelled.load(Ordering::Acquire) {
            let _ = fs::remove_dir_all(&checkout_dir);
            result = self.run_clone_with_progress(
                [
                    OsStr::new("clone"),
                    OsStr::new("--progress"),
                    OsStr::new("--"),
                    OsStr::new(remote_url),
                    checkout_dir.as_os_str(),
                ],
                cancelled,
                &mut on_progress,
            );
        }
        result?;

        if cancelled.load(Ordering::Acquire) {
            return Err(GitError::Cancelled);
        }

        let detected_branch = self
            .run(
                Some(&checkout_dir),
                "read branch",
                ["symbolic-ref", "--quiet", "--short", "HEAD"],
            )
            .ok()
            .and_then(|out| out.status.success().then(|| stdout(&out)))
            .unwrap_or_else(|| default_branch.to_string());
        let state = self.inspect(&checkout_dir, &detected_branch)?;
        let outcome = match state.local_revision.clone() {
            Some(revision)
                if matches!(
                    state.status,
                    CloneStatus::Current | CloneStatus::OtherBranch
                ) =>
            {
                SyncOutcome::Cloned { revision }
            }
            None if state.status == CloneStatus::Current => SyncOutcome::Cloned {
                revision: String::new(),
            },
            _ => return Ok(SyncOutcome::Skipped(state)),
        };

        // Re-check destination right before atomic swap.
        let final_kind = classify_destination(project_root, path);
        if final_kind == DestinationKind::EmptyDir {
            fs::remove_dir(path).map_err(|source| GitError::Io {
                operation: "remove empty destination directory",
                source,
            })?;
        } else if allow_replace_git_only
            && final_kind == DestinationKind::GitRepo
            && is_dot_git_only_dir(path)
        {
            let still_empty = self
                .inspect(path, default_branch)
                .is_ok_and(|s| s.local_revision.is_none() && s.remote_revision.is_none());
            if !still_empty {
                return Err(GitError::InvalidInput(
                    "repository destination changed during clone",
                ));
            }
            fs::remove_dir_all(path).map_err(|source| GitError::Io {
                operation: "remove incomplete git directory",
                source,
            })?;
        } else if final_kind != DestinationKind::Missing {
            return Err(GitError::InvalidInput(
                "repository destination became occupied during clone",
            ));
        }

        fs::rename(&checkout_dir, path).map_err(|source| GitError::Io {
            operation: "finalize cloned repository",
            source,
        })?;
        let _ = fs::remove_dir_all(&partial_dir);
        guard.disarm();
        Ok(outcome)
    }

    fn effective_limits(&self, operation: &str) -> (Option<Duration>, Option<Duration>) {
        let is_network = matches!(
            operation,
            "clone" | "fetch" | "fetch origin" | "verify remote origin" | "query remote branch"
        );
        if let Some(custom) = self.timeout {
            return (Some(custom), self.stall_timeout);
        }
        if is_network {
            (
                Some(DEFAULT_NETWORK_MAX_TIMEOUT),
                Some(self.stall_timeout.unwrap_or(DEFAULT_NETWORK_STALL_TIMEOUT)),
            )
        } else if matches!(operation, "fast-forward" | "set up local branch") {
            // Local working-tree writes must never be killed mid-checkout by a fixed timer,
            // regardless of repository size or disk speed.
            (None, None)
        } else {
            (Some(DEFAULT_LOCAL_GIT_TIMEOUT), None)
        }
    }

    fn run_clone_with_progress<I, S>(
        &self,
        args: I,
        cancelled: &AtomicBool,
        on_progress: &mut impl FnMut(CloneProgress),
    ) -> Result<(), GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if cancelled.load(Ordering::Acquire) {
            return Err(GitError::Cancelled);
        }
        let (hard_timeout, stall_timeout) = self.effective_limits("clone");
        let start = Instant::now();
        let last_activity_ms = Arc::new(std::sync::atomic::AtomicU64::new(0));
        perf::record_spawn();
        let mut child = self
            .command(None, args)?
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| GitError::Io {
                operation: "clone",
                source,
            })?;
        let stderr = child.stderr.take();
        let (tx, rx) = std::sync::mpsc::channel::<CloneProgress>();
        let activity_err = last_activity_ms.clone();
        let reader_handle = stderr.map(|mut pipe| {
            std::thread::spawn(move || {
                let mut buf = [0u8; 512];
                let mut pending = String::new();
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    activity_err.store(start.elapsed().as_millis() as u64, Ordering::Release);
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    pending.push_str(&chunk);
                    while let Some(pos) = pending.find(['\r', '\n']) {
                        let line = pending[..pos].to_string();
                        pending.drain(..=pos);
                        if let Some(progress) = parse_git_progress(&line) {
                            let _ = tx.send(progress);
                        }
                    }
                }
                if let Some(progress) = parse_git_progress(&pending) {
                    let _ = tx.send(progress);
                }
            })
        });

        loop {
            while let Ok(p) = rx.try_recv() {
                on_progress(p);
            }
            if cancelled.load(Ordering::Acquire) {
                kill_process_group(&mut child, self.termination_grace);
                if let Some(h) = reader_handle {
                    let _ = h.join();
                }
                return Err(GitError::Cancelled);
            }
            let elapsed = start.elapsed();
            let idle = elapsed.saturating_sub(Duration::from_millis(
                last_activity_ms.load(Ordering::Acquire),
            ));
            let stalled = stall_timeout.is_some_and(|st| idle >= st);
            let timed_out = hard_timeout.is_some_and(|ht| elapsed >= ht);
            if timed_out || stalled {
                kill_process_group(&mut child, self.termination_grace);
                if let Some(h) = reader_handle {
                    let _ = h.join();
                }
                return Err(GitError::TimedOut {
                    operation: "clone",
                    timeout: if stalled {
                        stall_timeout.or(hard_timeout).unwrap_or(Duration::ZERO)
                    } else {
                        hard_timeout.unwrap_or(Duration::ZERO)
                    },
                });
            }
            if let Some(status) = child.try_wait().map_err(|source| GitError::Io {
                operation: "clone",
                source,
            })? {
                if let Some(h) = reader_handle {
                    let _ = h.join();
                }
                while let Ok(p) = rx.try_recv() {
                    on_progress(p);
                }
                if cancelled.load(Ordering::Acquire) {
                    return Err(GitError::Cancelled);
                }
                if status.success() {
                    return Ok(());
                }
                return Err(GitError::Command {
                    operation: "clone",
                    code: status.code(),
                    detail: None,
                });
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Fetch and update an existing checkout; never create a missing checkout.
    pub fn sync_existing(
        &self,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
    ) -> Result<SyncOutcome, GitError> {
        self.sync_inner(remote_url, path, default_branch, false)
    }

    /// Fetch and fast-forward an existing checkout with cancellable Git subprocesses.
    pub fn sync_existing_cancellable(
        &self,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
        cancelled: &AtomicBool,
        mut progress: impl FnMut(&'static str),
    ) -> Result<SyncOutcome, GitError> {
        validate_branch(default_branch)?;
        self.validate_credential_remote(remote_url)?;
        if remote_url.is_empty()
            || remote_url.starts_with('-')
            || remote_url.chars().any(char::is_control)
            || has_embedded_credentials(remote_url)
        {
            return Err(GitError::InvalidInput("remote URL is empty or unsafe"));
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(GitError::Cancelled);
        }
        if !path.exists() {
            return Ok(SyncOutcome::Skipped(CloneState::new(
                CloneStatus::NotCloned,
            )));
        }

        progress("Checking local changes");
        let before = self.inspect(path, default_branch)?;
        if before.status == CloneStatus::Error
            && !(before.local_revision.is_none() && before.remote_revision.is_some())
        {
            return Ok(SyncOutcome::Skipped(before));
        }
        let origin = self.run(Some(path), "read origin", ["remote", "get-url", "origin"])?;
        if !origin.status.success() || stdout(&origin) != remote_url {
            let mut state = before;
            state.status = CloneStatus::Error;
            state.message = Some("Clone origin does not match the expected remote".into());
            return Ok(SyncOutcome::Skipped(state));
        }

        progress("Fetching remote");
        self.run_cancellable(
            Some(path),
            "fetch",
            ["fetch", "--prune", "origin"],
            cancelled,
        )?;
        progress("Comparing revisions");
        let state = self.inspect(path, default_branch)?;
        if state.status == CloneStatus::Error
            && state.local_revision.is_none()
            && let Some(remote_revision) = state.remote_revision.as_ref()
        {
            if cancelled.load(Ordering::Acquire) {
                return Err(GitError::Cancelled);
            }
            let remote_ref = format!("origin/{default_branch}");
            success(
                "set up local branch",
                self.run(
                    Some(path),
                    "set up local branch",
                    [
                        "switch",
                        "-C",
                        default_branch,
                        "--track",
                        remote_ref.as_str(),
                    ],
                )?,
            )?;
            return Ok(SyncOutcome::FastForwarded {
                from: String::new(),
                to: remote_revision.clone(),
            });
        }
        match state.status {
            CloneStatus::Current => Ok(SyncOutcome::AlreadyCurrent),
            CloneStatus::Behind => {
                let from = state
                    .local_revision
                    .clone()
                    .ok_or(GitError::InvalidInput("missing local revision"))?;
                let to = state
                    .remote_revision
                    .clone()
                    .ok_or(GitError::InvalidInput("missing remote revision"))?;
                if cancelled.load(Ordering::Acquire) {
                    return Err(GitError::Cancelled);
                }
                let before_merge = self.inspect(path, default_branch)?;
                if before_merge.status != CloneStatus::Behind {
                    return Ok(SyncOutcome::Skipped(before_merge));
                }
                let locks = detect_stale_locks(path);
                if !locks.is_empty() {
                    let names: Vec<String> = locks
                        .iter()
                        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .collect();
                    return Err(GitError::Command {
                        operation: "fast-forward",
                        code: Some(1),
                        detail: Some(format!(
                            "blocked by Git lock file ({}); remove stale lock or wait for the active Git process to finish",
                            names.join(", ")
                        )),
                    });
                }
                progress("Applying fast-forward");
                let remote_ref = format!("refs/remotes/origin/{default_branch}");
                success(
                    "fast-forward",
                    self.run(
                        Some(path),
                        "fast-forward",
                        ["merge", "--ff-only", "--", remote_ref.as_str()],
                    )?,
                )?;
                Ok(SyncOutcome::FastForwarded { from, to })
            }
            _ => Ok(SyncOutcome::Skipped(state)),
        }
    }

    fn sync_inner(
        &self,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
        allow_clone: bool,
    ) -> Result<SyncOutcome, GitError> {
        validate_branch(default_branch)?;
        self.validate_credential_remote(remote_url)?;
        if remote_url.is_empty()
            || remote_url.starts_with('-')
            || remote_url.chars().any(char::is_control)
            || has_embedded_credentials(remote_url)
        {
            return Err(GitError::InvalidInput("remote URL is empty or unsafe"));
        }
        if !path.exists() {
            if !allow_clone {
                return Ok(SyncOutcome::Skipped(CloneState::new(
                    CloneStatus::NotCloned,
                )));
            }
            let parent = path
                .parent()
                .ok_or(GitError::InvalidInput("clone path has no parent"))?;
            fs::create_dir_all(parent).map_err(|source| GitError::Io {
                operation: "create clone parent",
                source,
            })?;
            let output = self.run(
                None,
                "clone",
                [
                    OsStr::new("clone"),
                    OsStr::new("--branch"),
                    OsStr::new(default_branch),
                    OsStr::new("--"),
                    OsStr::new(remote_url),
                    path.as_os_str(),
                ],
            )?;
            success("clone", output)?;
            let state = self.inspect(path, default_branch)?;
            return match state.local_revision {
                Some(revision) if state.status == CloneStatus::Current => {
                    Ok(SyncOutcome::Cloned { revision })
                }
                _ => Ok(SyncOutcome::Skipped(state)),
            };
        }
        // Reject unrelated/non-repository directories before invoking fetch.
        let before = self.inspect(path, default_branch)?;
        if before.status == CloneStatus::Error
            && !(before.local_revision.is_none() && before.remote_revision.is_some())
        {
            return Ok(SyncOutcome::Skipped(before));
        }
        let origin = self.run(Some(path), "read origin", ["remote", "get-url", "origin"])?;
        if !origin.status.success() || stdout(&origin) != remote_url {
            let mut state = before;
            state.status = CloneStatus::Error;
            state.message = Some("Clone origin does not match the expected remote".into());
            return Ok(SyncOutcome::Skipped(state));
        }
        self.fetch_origin(path)?;
        let state = self.inspect(path, default_branch)?;
        if state.status == CloneStatus::Error
            && state.local_revision.is_none()
            && let Some(remote_revision) = state.remote_revision.as_ref()
        {
            let remote_ref = format!("origin/{default_branch}");
            let output = self.run(
                Some(path),
                "set up local branch",
                [
                    "switch",
                    "-C",
                    default_branch,
                    "--track",
                    remote_ref.as_str(),
                ],
            )?;
            success("set up local branch", output)?;
            return Ok(SyncOutcome::FastForwarded {
                from: String::new(),
                to: remote_revision.clone(),
            });
        }
        match state.status {
            CloneStatus::Current => Ok(SyncOutcome::AlreadyCurrent),
            CloneStatus::Behind => {
                let from = state
                    .local_revision
                    .clone()
                    .ok_or(GitError::InvalidInput("missing local revision"))?;
                let to = state
                    .remote_revision
                    .clone()
                    .ok_or(GitError::InvalidInput("missing remote revision"))?;
                // Inspect once more immediately before merge to reduce accidental updates after edits.
                if self.inspect(path, default_branch)?.status != CloneStatus::Behind {
                    return Ok(SyncOutcome::Skipped(self.inspect(path, default_branch)?));
                }
                let remote_ref = format!("refs/remotes/origin/{default_branch}");
                let output = self.run(
                    Some(path),
                    "fast-forward",
                    ["merge", "--ff-only", "--", remote_ref.as_str()],
                )?;
                success("fast-forward", output)?;
                Ok(SyncOutcome::FastForwarded { from, to })
            }
            _ => Ok(SyncOutcome::Skipped(state)),
        }
    }

    /// Read recent commits from a local clone.
    pub fn recent_commits(&self, path: &Path, count: usize) -> Result<Vec<GitCommit>, GitError> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let count_arg = format!("-{}", count.clamp(1, 100));
        let output = self.run(
            Some(path),
            "read commit log",
            ["log", &count_arg, "--format=%H%x1f%h%x1f%an%x1f%cr%x1f%s"],
        )?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        let out = stdout(&output);
        let commits = out
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\x1f').collect();
                if parts.len() >= 5 {
                    Some(GitCommit {
                        hash: parts[0].to_string(),
                        short_hash: parts[1].to_string(),
                        author_name: parts[2].to_string(),
                        date: parts[3].to_string(),
                        message: parts[4].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();
        Ok(commits)
    }

    fn run<I, S>(
        &self,
        path: Option<&Path>,
        operation: &'static str,
        args: I,
    ) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let t = perf::start();
        let (hard_timeout, stall_timeout) = self.effective_limits(operation);
        let output = self.run_with_limits(path, operation, args, None, hard_timeout, stall_timeout);
        perf::record_subprocess(
            operation,
            t,
            output.as_ref().ok().and_then(|o| o.status.code()),
        );
        output
    }

    fn run_cancellable<I, S>(
        &self,
        path: Option<&Path>,
        operation: &'static str,
        args: I,
        cancelled: &AtomicBool,
    ) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (hard_timeout, stall_timeout) = self.effective_limits(operation);
        self.run_with_limits(
            path,
            operation,
            args,
            Some(cancelled),
            hard_timeout,
            stall_timeout,
        )
    }

    fn run_with_limits<I, S>(
        &self,
        path: Option<&Path>,
        operation: &'static str,
        args: I,
        cancelled: Option<&AtomicBool>,
        hard_timeout: Option<Duration>,
        stall_timeout: Option<Duration>,
    ) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err(GitError::Cancelled);
        }
        let start = Instant::now();
        let last_activity_ms = Arc::new(std::sync::atomic::AtomicU64::new(0));
        perf::record_spawn();
        let mut child = self
            .command(path, args)?
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| GitError::Io { operation, source })?;
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let act_out = last_activity_ms.clone();
        let out_handle = stdout_pipe.map(|mut p| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while let Ok(n) = p.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    act_out.store(start.elapsed().as_millis() as u64, Ordering::Release);
                    buf.extend_from_slice(&chunk[..n]);
                }
                buf
            })
        });
        let act_err = last_activity_ms.clone();
        let err_handle = stderr_pipe.map(|mut p| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while let Ok(n) = p.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    act_err.store(start.elapsed().as_millis() as u64, Ordering::Release);
                    buf.extend_from_slice(&chunk[..n]);
                }
                buf
            })
        });

        let mut poll_ms = 1u64;
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|source| GitError::Io { operation, source })?
            {
                let stdout = out_handle.and_then(|h| h.join().ok()).unwrap_or_default();
                let stderr = err_handle.and_then(|h| h.join().ok()).unwrap_or_default();
                if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
                    return Err(GitError::Cancelled);
                }
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
                kill_process_group(&mut child, self.termination_grace);
                let _ = out_handle.map(|h| h.join());
                let _ = err_handle.map(|h| h.join());
                return Err(GitError::Cancelled);
            }
            let elapsed = start.elapsed();
            let idle = elapsed.saturating_sub(Duration::from_millis(
                last_activity_ms.load(Ordering::Acquire),
            ));
            let stalled = stall_timeout.is_some_and(|st| idle >= st);
            let timed_out = hard_timeout.is_some_and(|ht| elapsed >= ht);
            if timed_out || stalled {
                kill_process_group(&mut child, self.termination_grace);
                let _ = out_handle.map(|h| h.join());
                let _ = err_handle.map(|h| h.join());
                return Err(GitError::TimedOut {
                    operation,
                    timeout: if stalled {
                        stall_timeout.or(hard_timeout).unwrap_or(Duration::ZERO)
                    } else {
                        hard_timeout.unwrap_or(Duration::ZERO)
                    },
                });
            }
            std::thread::sleep(Duration::from_millis(poll_ms));
            poll_ms = (poll_ms * 2).min(20);
        }
    }

    fn command<I, S>(&self, path: Option<&Path>, args: I) -> Result<Command, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = if self.low_io_priority {
            background_io_command(&self.executable)
        } else {
            Command::new(&self.executable)
        };
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            command.process_group(0);
        }
        if let Some(credential) = &self.credential {
            let helper = format!(
                "!{} --git-credential-helper",
                shell_quote(
                    credential
                        .helper_executable
                        .to_str()
                        .ok_or(GitError::InvalidInput("invalid helper path"))?
                )
            );
            command
                .args([
                    "-c",
                    "credential.helper=",
                    "-c",
                    &format!("credential.helper={helper}"),
                    "-c",
                    "credential.useHttpPath=true",
                    "-c",
                    "http.followRedirects=false",
                    "-c",
                    "http.extraHeader=",
                    "-c",
                    "credential.interactive=never",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .env("REPO_MANAGER_CREDENTIAL_REF", &credential.credential_ref)
                .env("REPO_MANAGER_CREDENTIAL_HOST", &credential.host)
                .env("REPO_MANAGER_CREDENTIAL_PATH", &credential.path)
                .env("REPO_MANAGER_CREDENTIAL_USERNAME", &credential.username)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env_remove("GIT_CONFIG_COUNT")
                .env("GIT_ASKPASS", "")
                .env("SSH_ASKPASS", "");
            for key in [
                "GIT_TRACE",
                "GIT_TRACE_CURL",
                "GIT_TRACE_CURL_NO_DATA",
                "GIT_TRACE_PACKET",
                "GIT_TRACE2",
                "GIT_TRACE2_EVENT",
                "GIT_TRACE2_PERF",
            ] {
                command.env_remove(key);
            }
        } else if self.credential_manager {
            command
                .args([
                    "-c",
                    "credential.helper=",
                    "-c",
                    "credential.helper=!git-credential-manager",
                    "-c",
                    "credential.useHttpPath=true",
                    "-c",
                    "http.followRedirects=false",
                    "-c",
                    "http.extraHeader=",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null");
        } else if self.codecommit_credentials {
            command
                .args([
                    "-c",
                    "credential.helper=",
                    "-c",
                    "credential.helper=!aws codecommit credential-helper $@",
                    "-c",
                    "credential.useHttpPath=true",
                    "-c",
                    "credential.interactive=never",
                    "-c",
                    "http.followRedirects=false",
                ])
                .env("GIT_TERMINAL_PROMPT", "0");
        } else if let Some(token) = &self.token {
            let b64 = to_base64(format!("x-access-token:{token}").as_bytes());
            let header = format!("http.extraHeader=Authorization: Basic {b64}");
            command
                .args(["-c", "credential.interactive=never", "-c", &header])
                .env("GIT_TERMINAL_PROMPT", "0");
        }
        command
            .args(args)
            .stdin(Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0");
        if let Some(path) = path {
            command.current_dir(path);
        }
        Ok(command)
    }
}

/// Platform abstraction to run background git subprocesses at throttled I/O priority.
fn background_io_command(executable: &OsStr) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("/usr/sbin/taskpolicy");
        cmd.arg("-b").arg(executable);
        cmd
    }
    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new("ionice");
        cmd.args(["-c", "3"]).arg(executable);
        cmd
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Command::new(executable)
    }
}

/// Gracefully send SIGTERM to the entire process group, wait up to `grace`, then escalate to SIGKILL.
fn kill_process_group(child: &mut Child, grace: Duration) {
    #[cfg(unix)]
    {
        let pgid = child.id();
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &format!("-{pgid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = Command::new("/bin/kill")
            .args(["-KILL", &format!("-{pgid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn to_base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn validate_branch(branch: &str) -> Result<(), GitError> {
    if branch.is_empty()
        || branch.starts_with('-')
        || branch.starts_with('/')
        || branch.ends_with('/')
        || branch.contains("..")
        || branch.contains("@{")
        || branch.contains("//")
        || branch
            .split('/')
            .any(|part| part == "." || part == ".." || part.ends_with(".lock"))
        || branch
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
    {
        return Err(GitError::InvalidInput("invalid default branch"));
    }
    Ok(())
}

fn has_embedded_credentials(url: &str) -> bool {
    if let Some((scheme, after_scheme)) = url.split_once("://") {
        let authority = after_scheme.split('/').next().unwrap_or_default();
        if let Some((user, _)) = authority.split_once('@')
            && (scheme != "ssh" || user != "git")
        {
            return true;
        }
        return url.contains('?') || url.contains('#');
    }
    url.contains('@') && !url.starts_with("git@")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn paths_match(root: &str, requested: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else {
        return false;
    };
    let Ok(requested) = fs::canonicalize(requested) else {
        return false;
    };
    root == requested
}

fn command_error(operation: &'static str, output: &Output) -> GitError {
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let raw_line = stderr_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("hint:"))
        .map(|line| {
            line.strip_prefix("fatal:")
                .or_else(|| line.strip_prefix("error:"))
                .map(str::trim)
                .unwrap_or(line)
        })
        .unwrap_or("");
    let detail = if raw_line.is_empty() {
        None
    } else {
        let mut sanitized = redact_url_credentials(raw_line);
        if sanitized.len() > 220 {
            sanitized.truncate(220);
            sanitized.push('…');
        }
        Some(sanitized)
    };
    GitError::Command {
        operation,
        code: output.status.code(),
        detail,
    }
}

fn success(operation: &'static str, output: Output) -> Result<Output, GitError> {
    if output.status.success() {
        Ok(output)
    } else {
        Err(command_error(operation, &output))
    }
}

#[cfg(test)]
pub(crate) fn tests_fixture_temp_dir(tag: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("release-desk-git-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

struct PorcelainV2Summary {
    head_sha: Option<String>,
    branch: Option<String>,
    detached: bool,
    upstream: Option<String>,
    ab: Option<(u32, u32)>,
    dirty_tracked: bool,
    dirty_any: bool,
    dirty_tracked_count: usize,
    dirty_untracked_count: usize,
    sample_dirty_paths: Vec<String>,
}

impl PorcelainV2Summary {
    fn parse(text: &str) -> Self {
        let mut out = Self {
            head_sha: None,
            branch: None,
            detached: false,
            upstream: None,
            ab: None,
            dirty_tracked: false,
            dirty_any: false,
            dirty_tracked_count: 0,
            dirty_untracked_count: 0,
            sample_dirty_paths: Vec::new(),
        };
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# branch.oid ") {
                let sha = rest.trim();
                if sha != "(initial)" && !sha.is_empty() {
                    out.head_sha = Some(sha.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("# branch.head ") {
                let head = rest.trim();
                if head == "(detached)" {
                    out.detached = true;
                } else if !head.is_empty() {
                    out.branch = Some(head.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
                let up = rest.trim();
                if !up.is_empty() {
                    out.upstream = Some(up.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
                let mut parts = rest.split_whitespace();
                let ahead = parts
                    .next()
                    .and_then(|s| s.strip_prefix('+'))
                    .and_then(|s| s.parse::<u32>().ok());
                let behind = parts
                    .next()
                    .and_then(|s| s.strip_prefix('-'))
                    .and_then(|s| s.parse::<u32>().ok());
                if let (Some(a), Some(b)) = (ahead, behind) {
                    out.ab = Some((a, b));
                }
            } else if !line.is_empty() && !line.starts_with('#') {
                if !line.starts_with("! ") {
                    out.dirty_any = true;
                }
                if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
                    out.dirty_tracked = true;
                    out.dirty_tracked_count += 1;
                    if out.sample_dirty_paths.len() < 3
                        && let Some(path) = line.split('\t').next().and_then(|head| {
                            if line.starts_with("2 ") {
                                head.splitn(10, ' ').nth(9)
                            } else if line.starts_with("u ") {
                                head.splitn(11, ' ').nth(10)
                            } else {
                                head.splitn(9, ' ').nth(8)
                            }
                        })
                        && !path.is_empty()
                    {
                        out.sample_dirty_paths.push(path.to_string());
                    }
                } else if let Some(path) = line.strip_prefix("? ") {
                    out.dirty_untracked_count += 1;
                    if out.sample_dirty_paths.len() < 3 && !path.is_empty() {
                        out.sample_dirty_paths.push(path.to_string());
                    }
                }
            }
        }
        out
    }

    fn dirty_detail_message(&self, orig_head_present: bool) -> String {
        let mut parts = Vec::new();
        if self.dirty_tracked_count > 0 {
            let noun = if self.dirty_tracked_count == 1 {
                "tracked file"
            } else {
                "tracked files"
            };
            parts.push(format!(
                "{} modified/deleted {noun}",
                self.dirty_tracked_count
            ));
        }
        if self.dirty_untracked_count > 0 {
            let noun = if self.dirty_untracked_count == 1 {
                "untracked file"
            } else {
                "untracked files"
            };
            parts.push(format!("{} {noun}", self.dirty_untracked_count));
        }
        let mut msg = if parts.is_empty() {
            "Commit or stash local modifications before pulling updates.".to_string()
        } else if self.sample_dirty_paths.is_empty() {
            format!("{} in working tree.", parts.join(", "))
        } else {
            format!(
                "{} (e.g. {}).",
                parts.join(", "),
                self.sample_dirty_paths.join(", ")
            )
        };
        if orig_head_present && self.dirty_tracked_count >= 10 {
            msg.push_str(
                " Interrupted merge/checkout detected (ORIG_HEAD present; working tree differs from HEAD).",
            );
        }
        msg
    }
}

fn destination_error_state(kind: DestinationKind) -> Option<CloneState> {
    let mut state = CloneState::new(CloneStatus::Error);
    match kind {
        DestinationKind::Missing => Some(CloneState::new(CloneStatus::NotCloned)),
        DestinationKind::EmptyDir => {
            state.message = Some("Empty directory at repository destination".into());
            Some(state)
        }
        DestinationKind::NonGitDir => {
            state.message = Some("Path is not a Git working tree".into());
            Some(state)
        }
        DestinationKind::WrongRoot { .. } => {
            state.message = Some("Path is inside another Git working tree".into());
            Some(state)
        }
        DestinationKind::Symlink(_) => {
            state.message = Some("Repository path is a symbolic link".into());
            Some(state)
        }
        DestinationKind::Unreadable(error) => {
            state.message = Some(error);
            Some(state)
        }
        DestinationKind::GitRepo => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        remote: PathBuf,
        clone: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "release-desk-git-test-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).expect("create test directory");
            let source = root.join("source");
            let remote = root.join("remote.git");
            let clone = root.join("clone");
            git(
                &root,
                &["init", "--bare", remote.to_str().expect("UTF-8 test path")],
            );
            git(
                &root,
                &[
                    "init",
                    "-b",
                    "main",
                    source.to_str().expect("UTF-8 test path"),
                ],
            );
            git(&source, &["config", "user.name", "Test"]);
            git(&source, &["config", "user.email", "test@example.invalid"]);
            fs::write(source.join("README.md"), "initial\n").expect("write initial file");
            git(&source, &["add", "README.md"]);
            git(&source, &["commit", "-m", "initial"]);
            git(
                &source,
                &[
                    "remote",
                    "add",
                    "origin",
                    remote.to_str().expect("UTF-8 test path"),
                ],
            );
            git(&source, &["push", "-u", "origin", "main"]);
            git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
            Self {
                root,
                source,
                remote,
                clone,
            }
        }

        fn clone(&self) {
            let result = GitClient::new()
                .sync(
                    self.remote.to_str().expect("UTF-8 test path"),
                    &self.clone,
                    "main",
                )
                .expect("clone");
            assert!(matches!(result, SyncOutcome::Cloned { .. }));
            git(&self.clone, &["config", "user.name", "Test"]);
            git(
                &self.clone,
                &["config", "user.email", "test@example.invalid"],
            );
        }

        fn push_new_commit(&self) {
            fs::write(self.source.join("remote.txt"), "remote change\n")
                .expect("write remote file");
            git(&self.source, &["add", "remote.txt"]);
            git(&self.source, &["commit", "-m", "remote change"]);
            git(&self.source, &["push", "origin", "main"]);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git(path: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("run test Git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        stdout(&output)
    }

    #[test]
    fn absent_clone_and_clone_then_current() {
        let fixture = Fixture::new();
        let client = GitClient::new();
        assert_eq!(
            client
                .inspect(&fixture.clone, "main")
                .expect("inspect")
                .status,
            CloneStatus::NotCloned
        );
        fixture.clone();
        let state = client
            .inspect(&fixture.clone, "main")
            .expect("inspect clone");
        assert_eq!(state.status, CloneStatus::Current);
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!(state.local_revision, state.remote_revision);
    }

    #[test]
    fn fetch_and_fast_forward_clean_default_branch() {
        let fixture = Fixture::new();
        fixture.clone();
        fixture.push_new_commit();
        let client = GitClient::new();
        client.fetch(&fixture.clone).expect("fetch");
        let before = client.inspect(&fixture.clone, "main").expect("inspect");
        assert_eq!(before.status, CloneStatus::Behind);
        let outcome = client
            .sync(
                fixture.remote.to_str().expect("UTF-8 test path"),
                &fixture.clone,
                "main",
            )
            .expect("sync");
        assert!(matches!(outcome, SyncOutcome::FastForwarded { .. }));
        assert_eq!(
            client
                .inspect(&fixture.clone, "main")
                .expect("inspect")
                .status,
            CloneStatus::Current
        );
        assert!(fixture.clone.join("remote.txt").exists());
    }

    #[test]
    fn cancellable_sync_reports_stages_and_fast_forwards_only_after_fetch() {
        let fixture = Fixture::new();
        fixture.clone();
        fixture.push_new_commit();
        let client = GitClient::new();
        let cancelled = AtomicBool::new(false);
        let mut stages = Vec::new();
        let outcome = client
            .sync_existing_cancellable(
                fixture.remote.to_str().expect("UTF-8 test path"),
                &fixture.clone,
                "main",
                &cancelled,
                |stage| stages.push(stage),
            )
            .expect("cancellable sync");

        assert!(matches!(outcome, SyncOutcome::FastForwarded { .. }));
        assert_eq!(
            stages,
            [
                "Checking local changes",
                "Fetching remote",
                "Comparing revisions",
                "Applying fast-forward"
            ]
        );
        assert_eq!(
            client
                .inspect(&fixture.clone, "main")
                .expect("inspect")
                .status,
            CloneStatus::Current
        );
    }

    #[test]
    fn cancellable_sync_stops_before_running_git_when_cancelled() {
        let fixture = Fixture::new();
        fixture.clone();
        fixture.push_new_commit();
        let original = git(&fixture.clone, &["rev-parse", "HEAD"]);
        let cancelled = AtomicBool::new(true);
        let result = GitClient::new().sync_existing_cancellable(
            fixture.remote.to_str().expect("UTF-8 test path"),
            &fixture.clone,
            "main",
            &cancelled,
            |_| {},
        );

        assert!(matches!(result, Err(GitError::Cancelled)));
        assert_eq!(git(&fixture.clone, &["rev-parse", "HEAD"]), original);
    }

    #[test]
    fn dirty_clone_is_not_fast_forwarded() {
        let fixture = Fixture::new();
        fixture.clone();
        let original = git(&fixture.clone, &["rev-parse", "HEAD"]);
        fs::write(fixture.clone.join("untracked.txt"), "local work\n").expect("write local file");
        fixture.push_new_commit();
        let result = GitClient::new()
            .sync(
                fixture.remote.to_str().expect("UTF-8 test path"),
                &fixture.clone,
                "main",
            )
            .expect("sync");
        assert!(matches!(
            result,
            SyncOutcome::Skipped(CloneState {
                status: CloneStatus::Dirty,
                ..
            })
        ));
        assert_eq!(git(&fixture.clone, &["rev-parse", "HEAD"]), original);
    }

    #[test]
    fn identifies_ahead_diverged_and_detached() {
        let fixture = Fixture::new();
        fixture.clone();
        fs::write(fixture.clone.join("local.txt"), "local\n").expect("write local file");
        git(&fixture.clone, &["add", "local.txt"]);
        git(&fixture.clone, &["commit", "-m", "local change"]);
        let client = GitClient::new();
        assert_eq!(
            client
                .inspect(&fixture.clone, "main")
                .expect("inspect")
                .status,
            CloneStatus::Ahead
        );
        fixture.push_new_commit();
        client.fetch(&fixture.clone).expect("fetch");
        assert_eq!(
            client
                .inspect(&fixture.clone, "main")
                .expect("inspect")
                .status,
            CloneStatus::Diverged
        );
        git(&fixture.clone, &["checkout", "--detach"]);
        assert_eq!(
            client
                .inspect(&fixture.clone, "main")
                .expect("inspect")
                .status,
            CloneStatus::Detached
        );
    }

    #[test]
    fn project_credentials_are_scoped_to_one_https_remote() {
        let executable = std::env::current_exe().expect("test executable");
        let context = GitCredentialContext::new(
            executable.clone(),
            "saved-account".into(),
            "https://github.com/owner/repo.git",
        )
        .expect("valid credential scope");
        let client = GitClient::new().with_credential(context);
        assert!(
            client
                .validate_credential_remote("https://github.com/owner/repo.git")
                .is_ok()
        );
        assert!(
            client
                .validate_credential_remote("https://github.com/other/repo.git")
                .is_err()
        );
        for remote in [
            "http://github.com/owner/repo.git",
            "https://user@github.com/owner/repo.git",
            "https://github.com/../repo.git",
            "https://evil.example/owner/repo.git",
        ] {
            assert!(
                GitCredentialContext::new(executable.clone(), "saved-account".into(), remote)
                    .is_err()
            );
        }
    }

    #[test]
    fn provider_credentials_require_the_configured_host_and_repository() {
        let executable = std::env::current_exe().expect("test executable");
        let remote = "https://git.example.com/team/app.git";
        let credential = GitCredentialContext::for_provider(
            executable.clone(),
            "saved-forge-token".into(),
            remote,
            "git.example.com",
            "developer",
        )
        .expect("forge credential scope");
        let client = GitClient::new().with_credential(credential);
        assert!(client.validate_credential_remote(remote).is_ok());
        assert!(
            client
                .validate_credential_remote("https://git.example.com/other/app.git")
                .is_err()
        );
        assert!(
            GitCredentialContext::for_provider(
                executable,
                "saved-forge-token".into(),
                "https://other.example.com/team/app.git",
                "git.example.com",
                "developer",
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_unrelated_directory_and_unsafe_branch() {
        let fixture = Fixture::new();
        let unrelated = fixture.root.join("unrelated");
        fs::create_dir(&unrelated).expect("create unrelated directory");
        let client = GitClient::new();
        assert_eq!(
            client.inspect(&unrelated, "main").expect("inspect").status,
            CloneStatus::Error
        );
        assert!(matches!(
            client.sync("-bad", &fixture.clone, "main"),
            Err(GitError::InvalidInput(_))
        ));
        assert!(matches!(
            client.inspect(&fixture.clone, "--upload-pack=evil"),
            Err(GitError::InvalidInput(_))
        ));
        assert!(matches!(
            client.sync(
                "https://token@github.com/owner/repo.git",
                &fixture.clone,
                "main"
            ),
            Err(GitError::InvalidInput(_))
        ));
    }

    #[test]
    fn refuses_to_fetch_unexpected_origin() {
        let fixture = Fixture::new();
        fixture.clone();
        let outcome = GitClient::new()
            .sync("/some/other/repository.git", &fixture.clone, "main")
            .expect("check origin");
        assert!(matches!(
            outcome,
            SyncOutcome::Skipped(CloneState {
                status: CloneStatus::Error,
                ..
            })
        ));
    }

    #[test]
    fn verified_fetch_checks_origin_and_reports_behind_without_updating() {
        let fixture = Fixture::new();
        fixture.clone();
        fixture.push_new_commit();
        let client = GitClient::new();
        let bad = client
            .fetch_verified("/wrong", &fixture.clone, "main")
            .expect("verify");
        assert_eq!(bad.status, CloneStatus::Error);
        let state = client
            .fetch_verified(
                fixture.remote.to_str().expect("path"),
                &fixture.clone,
                "main",
            )
            .expect("fetch");
        assert_eq!(state.status, CloneStatus::Behind);
        assert!(!fixture.clone.join("remote.txt").exists());
    }

    #[test]
    fn clone_only_refuses_existing_checkout_and_clones_into_empty_directory() {
        let fixture = Fixture::new();
        fixture.clone();
        fixture.push_new_commit();
        let client = GitClient::new();
        assert!(
            client
                .clone_only(
                    fixture.remote.to_str().expect("path"),
                    &fixture.clone,
                    "main"
                )
                .is_err()
        );
        assert!(!fixture.clone.join("remote.txt").exists());
        let empty = fixture.root.join("empty");
        fs::create_dir(&empty).expect("empty directory");
        let outcome = client
            .clone_only(fixture.remote.to_str().expect("path"), &empty, "main")
            .expect("empty dir should be clonable");
        assert!(matches!(outcome, SyncOutcome::Cloned { .. }));
        assert!(empty.join(".git").is_dir());
        assert!(empty.join("README.md").is_file());
    }

    #[test]
    fn destination_classifier_covers_every_path_state() {
        let fixture = Fixture::new();
        let missing = fixture.root.join("missing");
        assert_eq!(
            classify_destination(&fixture.root, &missing),
            DestinationKind::Missing
        );

        let empty = fixture.root.join("empty");
        fs::create_dir(&empty).unwrap();
        assert_eq!(
            classify_destination(&fixture.root, &empty),
            DestinationKind::EmptyDir
        );

        let non_git = fixture.root.join("nongit");
        fs::create_dir(&non_git).unwrap();
        fs::write(non_git.join("notes.txt"), "keep").unwrap();
        assert_eq!(
            classify_destination(&fixture.root, &non_git),
            DestinationKind::NonGitDir
        );

        fixture.clone();
        assert_eq!(
            classify_destination(&fixture.root, &fixture.clone),
            DestinationKind::GitRepo
        );

        let nested = fixture.clone.join("sub");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("file.txt"), "inside parent").unwrap();
        assert!(matches!(
            classify_destination(&fixture.root, &nested),
            DestinationKind::WrongRoot { .. }
        ));

        let outside = fixture.root.parent().unwrap().join("outside");
        assert!(matches!(
            classify_destination(&fixture.root, &outside),
            DestinationKind::Unreadable(_)
        ));

        #[cfg(unix)]
        {
            let broken = fixture.root.join("broken-link");
            std::os::unix::fs::symlink(fixture.root.join("nowhere"), &broken).unwrap();
            assert_eq!(
                classify_destination(&fixture.root, &broken),
                DestinationKind::Symlink(SymlinkState::Broken)
            );

            let valid = fixture.root.join("valid-link");
            std::os::unix::fs::symlink(&fixture.clone, &valid).unwrap();
            assert_eq!(
                classify_destination(&fixture.root, &valid),
                DestinationKind::Symlink(SymlinkState::Valid)
            );
        }
    }

    #[test]
    fn atomic_clone_cleans_partial_on_failure_cancel_panic_and_handles_empty_remote() {
        let fixture = Fixture::new();
        let client = GitClient::new();
        let target = fixture.root.join("owner").join("demo");

        // Failure leaves no target dir and no .partial dir
        assert!(
            client
                .clone_atomic_cancellable(
                    &fixture.root,
                    "/nonexistent/remote.git",
                    &target,
                    "main",
                    false,
                    &AtomicBool::new(false),
                    |_| {}
                )
                .is_err()
        );
        assert!(!target.exists());
        let parent = target.parent().unwrap();
        assert!(fs::read_dir(parent).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".partial")
        }));

        // Pre-cancelled clone leaves no .partial dir
        assert!(matches!(
            client.clone_atomic_cancellable(
                &fixture.root,
                fixture.remote.to_str().unwrap(),
                &target,
                "main",
                false,
                &AtomicBool::new(true),
                |_| {}
            ),
            Err(GitError::Cancelled)
        ));
        assert!(!target.exists());

        // Panic while PartialCloneGuard is armed cleans only marked dir
        let partial = parent.join("demo.partial-panic");
        fs::create_dir_all(&partial).unwrap();
        fs::write(partial.join(PARTIAL_MARKER), b"marker").unwrap();
        let _ = std::panic::catch_unwind(|| {
            let _guard = PartialCloneGuard::new(partial.clone());
            panic!("simulated panic");
        });
        assert!(!partial.exists());

        // Empty remote returns SyncOutcome::Cloned instead of Skipped
        let empty_remote = fixture.root.join("empty-remote.git");
        git(
            &fixture.root,
            &[
                "init",
                "--bare",
                "-b",
                "main",
                empty_remote.to_str().unwrap(),
            ],
        );
        let outcome = client
            .clone_atomic_cancellable(
                &fixture.root,
                empty_remote.to_str().unwrap(),
                &target,
                "main",
                false,
                &AtomicBool::new(false),
                |_| {},
            )
            .expect("empty remote should clone cleanly");
        assert_eq!(
            outcome,
            SyncOutcome::Cloned {
                revision: String::new()
            }
        );
        assert!(target.join(".git").is_dir());

        // Stale partial cleanup requires marker + dead PID + minimum age
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let stale_old_dead = parent.join("x.partial-dead-old");
        fs::create_dir_all(&stale_old_dead).unwrap();
        fs::write(
            stale_old_dead.join(PARTIAL_MARKER),
            format_partial_marker(999_999, now_secs.saturating_sub(600)),
        )
        .unwrap();

        let young_dead = parent.join("x.partial-dead-young");
        fs::create_dir_all(&young_dead).unwrap();
        fs::write(
            young_dead.join(PARTIAL_MARKER),
            format_partial_marker(999_999, now_secs),
        )
        .unwrap();

        let live_old = parent.join("x.partial-live-old");
        fs::create_dir_all(&live_old).unwrap();
        fs::write(
            live_old.join(PARTIAL_MARKER),
            format_partial_marker(std::process::id(), now_secs.saturating_sub(600)),
        )
        .unwrap();

        let user_dir = parent.join("user.partial-keep");
        fs::create_dir_all(&user_dir).unwrap();

        assert_eq!(
            cleanup_stale_partial_clones(std::slice::from_ref(&fixture.root)),
            1
        );
        assert!(!stale_old_dead.exists());
        assert!(young_dead.exists(), "young partial dir must not be deleted");
        assert!(
            live_old.exists(),
            "live-PID partial dir must not be deleted"
        );
        assert!(user_dir.exists(), "unmarked dir must not be deleted");
    }

    #[cfg(unix)]
    #[test]
    fn timeout_and_cancel_kill_entire_git_process_group() {
        let fixture = Fixture::new();
        let pid_file = fixture.root.join("child_sleep.pid");
        let alias = format!(
            "alias.hang=!sh -c 'sleep 30 & echo $! > {}; wait'",
            shell_quote(pid_file.to_str().unwrap())
        );

        // 1. Timeout kills parent AND spawned `sleep 30` grandchild.
        let timed = GitClient::new().with_timeout(Duration::from_millis(250));
        let err = timed
            .run(Some(&fixture.root), "test timeout", ["-c", &alias, "hang"])
            .unwrap_err();
        assert!(
            matches!(err, GitError::TimedOut { .. }),
            "expected TimedOut, got {err:?}"
        );
        std::thread::sleep(Duration::from_millis(80));
        let grandchild_pid: u32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            !is_pid_alive(grandchild_pid),
            "grandchild sleep process {grandchild_pid} survived timeout kill!"
        );

        // 2. Cancellation kills parent AND spawned `sleep 30` grandchild.
        let _ = fs::remove_file(&pid_file);
        let cancel = AtomicBool::new(false);
        let pid_file_wait = pid_file.clone();
        std::thread::scope(|s| {
            s.spawn(|| {
                for _ in 0..50 {
                    if pid_file_wait.exists() {
                        std::thread::sleep(Duration::from_millis(50));
                        cancel.store(true, Ordering::Release);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
            let client = GitClient::new().with_timeout(Duration::from_secs(10));
            let res = client.run_cancellable(
                Some(&fixture.root),
                "test cancel",
                ["-c", &alias, "hang"],
                &cancel,
            );
            assert!(matches!(res, Err(GitError::Cancelled)), "got {res:?}");
        });
        std::thread::sleep(Duration::from_millis(80));
        let cancelled_pid: u32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            !is_pid_alive(cancelled_pid),
            "grandchild sleep process {cancelled_pid} survived cancel kill!"
        );
    }

    #[test]
    fn redacts_url_credentials_in_logs() {
        let raw = "fatal: could not access 'https://alice:ghp_secret123@github.com/org/repo.git/'";
        let clean = redact_url_credentials(raw);
        assert!(!clean.contains("alice"));
        assert!(!clean.contains("ghp_secret123"));
        assert!(clean.contains("https://<redacted>@github.com/org/repo.git/"));
    }

    #[test]
    fn parses_git_clone_progress_percent() {
        let p = parse_git_progress("Receiving objects:  45% (10/22), 1.2 MiB | 3.0 MiB/s").unwrap();
        assert_eq!(p.percent, Some(45));
        assert!(parse_git_progress("   ").is_none());
    }

    #[test]
    fn clone_only_creates_new_checkout() {
        let fixture = Fixture::new();
        let result = GitClient::new()
            .clone_only(
                fixture.remote.to_str().expect("path"),
                &fixture.clone,
                "main",
            )
            .expect("clone");
        assert!(matches!(result, SyncOutcome::Cloned { .. }));
    }

    #[test]
    fn sync_existing_does_not_clone_missing_directory() {
        let fixture = Fixture::new();
        let outcome = GitClient::new()
            .sync_existing(
                fixture.remote.to_str().expect("path"),
                &fixture.clone,
                "main",
            )
            .expect("sync");
        assert!(matches!(
            outcome,
            SyncOutcome::Skipped(CloneState {
                status: CloneStatus::NotCloned,
                ..
            })
        ));
        assert!(!fixture.clone.exists());
    }

    #[test]
    fn empty_checkout_is_not_an_update_error() {
        let fixture = Fixture::new();
        let empty = fixture.root.join("empty-checkout");
        git(
            &fixture.root,
            &["init", "-b", "main", empty.to_str().unwrap()],
        );
        let state = GitClient::new().inspect(&empty, "main").unwrap();
        assert_eq!(state.status, CloneStatus::Current);
        assert!(state.local_revision.is_none());
        assert!(state.message.unwrap().contains("no commits"));
    }

    #[test]
    fn existing_unborn_branch_can_start_from_verified_origin() {
        let fixture = Fixture::new();
        let empty = fixture.root.join("empty-checkout");
        git(
            &fixture.root,
            &["init", "-b", "main", empty.to_str().unwrap()],
        );
        git(
            &empty,
            &["remote", "add", "origin", fixture.remote.to_str().unwrap()],
        );
        let outcome = GitClient::new()
            .sync_existing(fixture.remote.to_str().unwrap(), &empty, "main")
            .unwrap();
        assert!(matches!(outcome, SyncOutcome::FastForwarded { .. }));
        assert_eq!(
            GitClient::new().inspect(&empty, "main").unwrap().status,
            CloneStatus::Current
        );
        assert!(empty.join("README.md").is_file());
    }

    #[test]
    fn alternate_branch_is_preserved() {
        let fixture = Fixture::new();
        fixture.clone();
        git(&fixture.clone, &["switch", "-c", "topic"]);
        fixture.push_new_commit();
        let outcome = GitClient::new()
            .sync(
                fixture.remote.to_str().expect("UTF-8 test path"),
                &fixture.clone,
                "main",
            )
            .expect("sync");
        assert!(matches!(
            outcome,
            SyncOutcome::Skipped(CloneState {
                status: CloneStatus::OtherBranch,
                ..
            })
        ));
        assert_eq!(git(&fixture.clone, &["branch", "--show-current"]), "topic");
    }

    #[test]
    fn recent_commits_returns_log_entries() {
        let fixture = Fixture::new();
        fixture.clone();
        let commits = GitClient::new()
            .recent_commits(&fixture.clone, 10)
            .expect("recent commits");
        assert!(!commits.is_empty());
        assert_eq!(commits[0].message, "initial");
        assert!(!commits[0].short_hash.is_empty());
        assert!(!commits[0].hash.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn c5_sigterm_grace_progressing_transfer_and_fetch_kill_leave_no_locks() {
        let fixture = Fixture::new();
        fixture.clone();
        fixture.push_new_commit();

        // 1. Slow progressing transfer (emits stderr bytes every 80ms for ~800ms, exceeding 400ms stall)
        // MUST NOT be killed when progress bytes keep arriving before each 400ms silence window!
        let slow_script = fixture.root.join("git-slow-progress");
        let trace_log = fixture.root.join("slow_trace.log");
        let real_git = [
            "/opt/homebrew/bin/git",
            "/usr/local/bin/git",
            "/usr/bin/git",
        ]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or("git");
        let trace_s = trace_log.display();
        fs::write(
            &slow_script,
            format!(
                "#!/usr/bin/perl\nuse strict;\nexit 0 if $ENV{{WARM}};\nopen(my $log, '>>', \"{trace_s}\");\nselect((select($log), $|=1)[0]);\nselect(STDERR);\n$| = 1;\nprint $log \"started\\n\";\nfor my $i (1..10) {{\n  print STDERR \"remote: Compressing objects: $i\\n\";\n  print $log \"wrote $i\\n\";\n  select(undef, undef, undef, 0.08);\n}}\nprint $log \"exec git\\n\";\nexec(\"{real_git}\", @ARGV) or die $!;\n"
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = fs::metadata(&slow_script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&slow_script, perms).unwrap();
        }
        // Warm macOS first-exec verification on newly created script before timed run.
        let _ = Command::new(&slow_script).env("WARM", "1").output();
        let progressing_client = GitClient {
            executable: slow_script.into_os_string(),
            stall_timeout: Some(Duration::from_millis(400)),
            ..GitClient::new()
        };
        let res = progressing_client.fetch(&fixture.clone);
        let trace_contents = fs::read_to_string(&trace_log).unwrap_or_default();
        assert!(
            res.is_ok(),
            "progressing transfer must not be killed by stall timeout, got: {res:?}; trace:\n{trace_contents}"
        );

        // 2. Stalled transfer against a local remote killed cleanly via SIGTERM -> grace -> SIGKILL leaves no lock files.
        fs::write(fixture.source.join("remote2.txt"), "remote change 2\n").unwrap();
        git(&fixture.source, &["add", "remote2.txt"]);
        git(&fixture.source, &["commit", "-m", "remote change 2"]);
        git(&fixture.source, &["push", "origin", "main"]);
        let stalled_script = fixture.root.join("git-stalled-fetch");
        fs::write(
            &stalled_script,
            format!(
                "#!/bin/sh\nif [ \"$WARM\" = \"1\" ]; then exit 0; fi\nif [ \"$1\" = \"fetch\" ]; then\n  sleep 30 & wait\n  exit 0\nfi\nexec \"{real_git}\" \"$@\"\n"
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = fs::metadata(&stalled_script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&stalled_script, perms).unwrap();
        }
        let _ = Command::new(&stalled_script).env("WARM", "1").output();
        let stalled_client = GitClient {
            executable: stalled_script.into_os_string(),
            stall_timeout: Some(Duration::from_millis(120)),
            termination_grace: Duration::from_millis(120),
            ..GitClient::new()
        };
        let err = stalled_client.fetch(&fixture.clone).unwrap_err();
        assert!(matches!(err, GitError::TimedOut { .. }));
        assert!(
            detect_stale_locks(&fixture.clone).is_empty(),
            "killing a stalled fetch must leave no stale .lock files behind"
        );

        // 3. Stale lock detection finds locks without removing them until explicit confirmation.
        let lock = fixture.clone.join(".git/index.lock");
        fs::write(&lock, "").unwrap();
        let ref_lock = fixture.clone.join(".git/refs/heads/main.lock");
        fs::write(&ref_lock, "").unwrap();
        let found = detect_stale_locks(&fixture.clone);
        assert_eq!(found.len(), 2);
        assert!(lock.exists() && ref_lock.exists());
        assert_eq!(remove_stale_locks_explicit(&fixture.clone).unwrap(), 2);
        assert!(detect_stale_locks(&fixture.clone).is_empty());
    }

    #[test]
    fn phase5_porcelain_v2_collapses_subprocesses_and_falls_back_on_custom_upstream() {
        perf::set_enabled(true);
        let fixture = Fixture::new();
        fixture.clone();
        let client = GitClient::new();

        // 1. Clean default branch tracking origin/main -> 1 subprocess total!
        let sub_before = perf::reset_subprocess_counter();
        let (state, ahead, behind, dirty) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        let (subs, _) = perf::subprocess_delta(sub_before);
        assert_eq!(state.status, CloneStatus::Current);
        assert_eq!((ahead, behind, dirty), (Some(0), Some(0), false));
        assert_eq!(subs, 1, "clean matching upstream must take 1 subprocess");

        // 2. Tracked change + untracked file -> dirty_tracked = true with -uno in 1 subprocess.
        fs::write(fixture.clone.join("untracked.tmp"), "ignore me\n").unwrap();
        let (s2, _, _, d2) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        assert!(
            !d2,
            "-uno must treat untracked-only file as clean tracked state"
        );
        assert_eq!(s2.status, CloneStatus::Current);
        fs::write(fixture.clone.join("README.md"), "modified\n").unwrap();
        let sub_before_d = perf::reset_subprocess_counter();
        let (s3, _, _, d3) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        assert!(d3);
        assert_eq!(s3.status, CloneStatus::Current);
        assert_eq!(perf::subprocess_delta(sub_before_d).0, 1);

        // 3. Unset/custom upstream falls back cleanly to rev-parse + rev-list against origin/main!
        git(&fixture.clone, &["checkout", "--", "README.md"]);
        git(&fixture.clone, &["branch", "--unset-upstream", "main"]);
        fixture.push_new_commit();
        git(&fixture.clone, &["fetch", "origin"]);
        let (s4, a4, b4, d4) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        assert_eq!(s4.status, CloneStatus::Behind);
        assert_eq!((a4, b4, d4), (Some(0), Some(1), false));

        // 4. fetch_verified on clean matching upstream uses <= 3 subprocesses (down from 12-15).
        git(&fixture.clone, &["merge", "--ff-only", "origin/main"]);
        git(
            &fixture.clone,
            &["branch", "--set-upstream-to=origin/main", "main"],
        );
        let sub_before_f = perf::reset_subprocess_counter();
        let fstate = client
            .fetch_verified(fixture.remote.to_str().unwrap(), &fixture.clone, "main")
            .unwrap();
        let (fsubs, _) = perf::subprocess_delta(sub_before_f);
        assert_eq!(fstate.status, CloneStatus::Current);
        assert!(
            fsubs <= 3,
            "fetch_verified should run <= 3 subprocesses, got {fsubs}"
        );
    }

    #[test]
    fn d5_porcelain_v2_dirty_detached_other_no_upstream_unborn_empty_remote() {
        let fixture = Fixture::new();
        fixture.clone();
        let client = GitClient::new();
        let remote_str = fixture.remote.to_str().unwrap();

        // 1. dirty+behind: tracked edit + upstream commit -> both inspect() and inspect_refs_and_dirty_uno()
        // MUST report remote_revision and ahead=Some(0), behind=Some(1) against origin/main.
        fixture.push_new_commit();
        git(&fixture.clone, &["fetch", "origin"]);
        fs::write(fixture.clone.join("README.md"), "dirty edit\n").unwrap();
        let (st_uno, a_uno, b_uno, dirty_uno) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        assert_eq!(st_uno.status, CloneStatus::Behind);
        assert_eq!((a_uno, b_uno, dirty_uno), (Some(0), Some(1), true));
        let (st_full, a_full, b_full, dirty_full) = client
            .inspect_porcelain_v2_inner(&fixture.clone, "main", true, true)
            .unwrap();
        assert_eq!(st_full.status, CloneStatus::Dirty);
        assert_eq!((a_full, b_full, dirty_full), (Some(0), Some(1), true));
        assert!(st_full.remote_revision.is_some());

        // 2. dirty+ahead: clean ff, make 1 local ahead commit + tracked edit.
        git(&fixture.clone, &["checkout", "--", "README.md"]);
        git(&fixture.clone, &["merge", "--ff-only", "origin/main"]);
        fs::write(fixture.clone.join("local.txt"), "local ahead\n").unwrap();
        git(&fixture.clone, &["add", "local.txt"]);
        git(&fixture.clone, &["commit", "-m", "local ahead"]);
        fs::write(fixture.clone.join("README.md"), "dirty ahead\n").unwrap();
        let (st_ua, a_ua, b_ua, dirty_ua) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        assert_eq!(st_ua.status, CloneStatus::Ahead);
        assert_eq!((a_ua, b_ua, dirty_ua), (Some(1), Some(0), true));
        let (st_fa, a_fa, b_fa, dirty_fa) = client
            .inspect_porcelain_v2_inner(&fixture.clone, "main", true, true)
            .unwrap();
        assert_eq!(st_fa.status, CloneStatus::Dirty);
        assert_eq!((a_fa, b_fa, dirty_fa), (Some(1), Some(0), true));

        // 3. detached HEAD (at HEAD~1 while origin/main is 1 behind HEAD -> 0 ahead, 0 behind origin/main)
        git(&fixture.clone, &["checkout", "--", "README.md"]);
        git(&fixture.clone, &["checkout", "--detach", "origin/main"]);
        let (st_det, a_det, b_det, _) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        assert_eq!(st_det.status, CloneStatus::Detached);
        assert!(st_det.branch.is_none());
        assert_eq!((a_det, b_det), (Some(0), Some(0)));

        // 4. non-default branch WITH its own tracked upstream `origin/feature` at +0 -0,
        // while `origin/main` moved ahead by 1 -> MUST measure against origin/main (behind=1), NOT origin/feature!
        git(&fixture.clone, &["switch", "-c", "feature"]);
        git(&fixture.clone, &["push", "-u", "origin", "feature"]);
        fs::write(fixture.source.join("remote2.txt"), "remote change 2\n").unwrap();
        git(&fixture.source, &["add", "remote2.txt"]);
        git(&fixture.source, &["commit", "-m", "remote change 2"]);
        git(&fixture.source, &["push", "origin", "main"]);
        git(&fixture.clone, &["fetch", "origin"]);
        let (st_oth, a_oth, b_oth, _) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        assert_eq!(st_oth.status, CloneStatus::OtherBranch);
        assert_eq!(st_oth.branch.as_deref(), Some("feature"));
        assert_eq!((a_oth, b_oth), (Some(0), Some(1)));

        // 5. no upstream on default branch `main` (reset main back to origin/main~1 and unset upstream)
        git(&fixture.clone, &["switch", "main"]);
        git(&fixture.clone, &["reset", "--hard", "origin/main~1"]);
        git(&fixture.clone, &["branch", "--unset-upstream", "main"]);
        let (st_nou, a_nou, b_nou, _) = client
            .inspect_refs_and_dirty_uno(&fixture.clone, "main")
            .unwrap();
        assert_eq!(st_nou.status, CloneStatus::Behind);
        assert_eq!((a_nou, b_nou), (Some(0), Some(1)));

        // 6. unborn branch with non-empty remote
        let unborn = fixture.root.join("unborn");
        git(
            &fixture.root,
            &["init", "-b", "main", unborn.to_str().unwrap()],
        );
        git(&unborn, &["remote", "add", "origin", remote_str]);
        git(&unborn, &["fetch", "origin"]);
        let (st_unb, a_unb, b_unb, _) = client.inspect_refs_and_dirty_uno(&unborn, "main").unwrap();
        assert_eq!(st_unb.status, CloneStatus::Error);
        assert!(st_unb.local_revision.is_none());
        assert!(st_unb.remote_revision.is_some());
        assert_eq!((a_unb, b_unb), (None, None));

        // 7a. empty remote + unborn local checkout
        let empty_local = fixture.root.join("empty-local");
        let empty_remote = fixture.root.join("empty-remote");
        git(
            &fixture.root,
            &["init", "--bare", empty_remote.to_str().unwrap()],
        );
        git(
            &fixture.root,
            &["init", "-b", "main", empty_local.to_str().unwrap()],
        );
        git(
            &empty_local,
            &["remote", "add", "origin", empty_remote.to_str().unwrap()],
        );
        let _ = client.fetch(&empty_local);
        let (st_emp, a_emp, b_emp, _) = client
            .inspect_refs_and_dirty_uno(&empty_local, "main")
            .unwrap();
        assert_eq!(st_emp.status, CloneStatus::Current);
        assert_eq!((a_emp, b_emp), (None, None));

        // 7b. committed local + empty remote -> Error ("online branch main is unavailable"), (None, None)
        fs::write(empty_local.join("first.txt"), "one\n").unwrap();
        git(&empty_local, &["add", "first.txt"]);
        git(&empty_local, &["commit", "-m", "first"]);
        let (st_emp2, a_emp2, b_emp2, _) = client
            .inspect_refs_and_dirty_uno(&empty_local, "main")
            .unwrap();
        assert_eq!(st_emp2.status, CloneStatus::Error);
        assert!(st_emp2.local_revision.is_some());
        assert!(st_emp2.remote_revision.is_none());
        assert_eq!((a_emp2, b_emp2), (None, None));
    }

    #[test]
    fn fast_forward_lock_preflight_redacted_stderr_and_dirty_detail_summary() {
        let fixture = Fixture::new();
        fixture.clone();
        fixture.push_new_commit();
        let remote_str = fixture.remote.to_str().unwrap();

        let client = GitClient::new();
        // 1. fast-forward and branch setup must never be killed by a fixed local timer.
        assert_eq!(client.effective_limits("fast-forward"), (None, None));
        assert_eq!(client.effective_limits("set up local branch"), (None, None));

        // 2. Stale `.git/index.lock` preflights cleanly with lock details before merge --ff-only.
        let lock = fixture.clone.join(".git").join("index.lock");
        fs::write(&lock, "").unwrap();
        let err = client
            .sync_existing_cancellable(
                remote_str,
                &fixture.clone,
                "main",
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("index.lock"),
            "expected lock error detail: {err}"
        );
        fs::remove_file(&lock).unwrap();

        // 3. Failed git stderr detail is captured and URL credentials are redacted.
        let fake_output = Output {
            status: Command::new("/usr/bin/false").status().unwrap(),
            stdout: Vec::new(),
            stderr: b"fatal: cannot access 'https://x-access-token:ghp_SECRET123@github.com/org/repo.git': failed\n".to_vec(),
        };
        let cmd_err = command_error("fetch", &fake_output).to_string();
        assert!(
            !cmd_err.contains("ghp_SECRET123"),
            "secret leaked: {cmd_err}"
        );
        assert!(
            cmd_err.contains("<redacted>@github.com"),
            "missing redaction: {cmd_err}"
        );

        // 4. PorcelainV2Summary surfaces modified file count + ORIG_HEAD note.
        let mut porcelain = String::from("# branch.oid abcdef1\n# branch.head main\n");
        for i in 0..12 {
            porcelain.push_str(&format!(
                "1 .M N... 100644 100644 100644 0000000 0000000 file_{i}.rs\n"
            ));
        }
        let parsed = PorcelainV2Summary::parse(&porcelain);
        let msg = parsed.dirty_detail_message(true);
        assert!(msg.contains("12 modified/deleted tracked files"), "{msg}");
        assert!(msg.contains("file_0.rs"), "{msg}");
        assert!(msg.contains("ORIG_HEAD"), "{msg}");
    }
}
