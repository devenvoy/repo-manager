//! System Git adapter for inspecting and updating managed repository clones.
//!
//! Every Git invocation passes separate arguments to [`Command`], never a shell.
//! Project credentials are supplied by a narrowly scoped, per-command helper.

pub mod adapter;
pub use adapter::SystemGitPort;

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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
    InvalidInput(&'static str),
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Command {
        operation: &'static str,
        code: Option<i32>,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(f, "invalid Git input: {message}"),
            Self::Io { operation, source } => write!(f, "Git {operation} failed: {source}"),
            Self::Command { operation, code } => {
                write!(f, "Git {operation} exited unsuccessfully ({code:?})")
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
        let rest = remote_url
            .strip_prefix("https://")
            .ok_or(GitError::InvalidInput(
                "project credentials require an HTTPS remote",
            ))?;
        let (host, path) = rest
            .split_once('/')
            .ok_or(GitError::InvalidInput("remote has no repository path"))?;
        let username = match host {
            "github.com" => "x-access-token",
            "gitlab.com" => "oauth2",
            _ => {
                return Err(GitError::InvalidInput(
                    "project credentials are not supported for this host",
                ));
            }
        };
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
        }
    }

    /// Use a selected credential for this exact remote; no token is stored here.
    pub fn with_credential(mut self, credential: GitCredentialContext) -> Self {
        self.credential = Some(credential);
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
        validate_branch(default_branch)?;
        if !path.exists() {
            return Ok(CloneState::new(CloneStatus::NotCloned));
        }
        let mut state = CloneState::new(CloneStatus::Error);
        let inside = self.run(
            Some(path),
            "inspect repository",
            ["rev-parse", "--is-inside-work-tree"],
        )?;
        if !inside.status.success() || stdout(&inside) != "true" {
            state.message = Some("Path is not a Git working tree".into());
            return Ok(state);
        }
        let root = self.run(
            Some(path),
            "inspect repository root",
            ["rev-parse", "--show-toplevel"],
        )?;
        if !root.status.success() || !paths_match(&stdout(&root), path) {
            state.message = Some("Path is inside another Git working tree".into());
            return Ok(state);
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
            return Ok(state);
        }
        state.branch = Some(stdout(&branch));
        let dirty = self.run(
            Some(path),
            "inspect changes",
            ["status", "--porcelain=v1", "--untracked-files=normal"],
        )?;
        if !dirty.status.success() {
            return Err(command_error("inspect changes", &dirty));
        }
        if !dirty.stdout.is_empty() {
            state.status = CloneStatus::Dirty;
            return Ok(state);
        }
        if state.branch.as_deref() != Some(default_branch) {
            state.status = CloneStatus::OtherBranch;
            return Ok(state);
        }
        let (Some(local), Some(remote)) = (&state.local_revision, &state.remote_revision) else {
            state.message = Some("Local or origin revision is unavailable".into());
            return Ok(state);
        };
        if local == remote {
            state.status = CloneStatus::Current;
        } else if self.is_ancestor(path, local, remote)? {
            state.status = CloneStatus::Behind;
        } else if self.is_ancestor(path, remote, local)? {
            state.status = CloneStatus::Ahead;
        } else {
            state.status = CloneStatus::Diverged;
        }
        Ok(state)
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
        let output = self.run(Some(path), "fetch", ["fetch", "--prune", "origin"])?;
        success("fetch", output).map(|_| ())
    }

    /// Fetch only after verifying that this directory is the expected checkout.
    pub fn fetch_verified(
        &self,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
    ) -> Result<CloneState, GitError> {
        self.validate_credential_remote(remote_url)?;
        let state = self.inspect(path, default_branch)?;
        if matches!(state.status, CloneStatus::NotCloned | CloneStatus::Error) {
            return Ok(state);
        }
        let origin = self.run(Some(path), "read origin", ["remote", "get-url", "origin"])?;
        if !origin.status.success() || stdout(&origin) != remote_url {
            let mut rejected = state;
            rejected.status = CloneStatus::Error;
            rejected.message = Some("Clone origin does not match the expected remote".into());
            return Ok(rejected);
        }
        self.fetch_origin(path)?;
        self.inspect(path, default_branch)
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

    /// Clone into a newly reserved directory, never fetching or updating an existing path.
    pub fn clone_only(
        &self,
        remote_url: &str,
        path: &Path,
        default_branch: &str,
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
        let parent = path
            .parent()
            .ok_or(GitError::InvalidInput("clone path has no parent"))?;
        fs::create_dir_all(parent).map_err(|source| GitError::Io {
            operation: "create clone parent",
            source,
        })?;
        // create_dir is atomic and fails for every existing path, including an empty directory.
        fs::create_dir(path).map_err(|source| GitError::Io {
            operation: "reserve clone destination",
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
        );
        match output.and_then(|output| success("clone", output)) {
            Ok(_) => {}
            Err(error) => {
                // Remove only an empty reservation. Never delete files left by Git or another process.
                let _ = fs::remove_dir(path);
                return Err(error);
            }
        }
        let state = self.inspect(path, default_branch)?;
        match state.local_revision.clone() {
            Some(revision) if state.status == CloneStatus::Current => {
                Ok(SyncOutcome::Cloned { revision })
            }
            _ => Ok(SyncOutcome::Skipped(state)),
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
        if before.status == CloneStatus::Error {
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

    fn is_ancestor(&self, path: &Path, older: &str, newer: &str) -> Result<bool, GitError> {
        let output = self.run(
            Some(path),
            "compare revisions",
            ["merge-base", "--is-ancestor", older, newer],
        )?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(command_error("compare revisions", &output)),
        }
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
        let mut command = Command::new(&self.executable);
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
                    "http.extraHeader=",
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
        }
        command
            .args(args)
            .stdin(Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0");
        if let Some(path) = path {
            command.current_dir(path);
        }
        command
            .output()
            .map_err(|source| GitError::Io { operation, source })
    }
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
    GitError::Command {
        operation,
        code: output.status.code(),
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
    fn clone_only_refuses_existing_checkout_and_empty_directory() {
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
        assert!(
            client
                .clone_only(fixture.remote.to_str().expect("path"), &empty, "main")
                .is_err()
        );
        assert!(empty.is_dir());
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
}
