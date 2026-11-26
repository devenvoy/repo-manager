//! Pure-stat Git reference fingerprinting (zero subprocesses).
//!
//! Resolves `.git` directories, `.git`-as-file worktrees/submodules, `commondir`,
//! loose refs, `packed-refs`, `FETCH_HEAD`, and `reftable` storage.
//! Detects reference changes only; working-tree edits are checked separately.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Metadata stamp for one file or directory captured via `symlink_metadata`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileStamp {
    pub mtime_secs: u64,
    pub mtime_nanos: u32,
    pub len: u64,
}

impl FileStamp {
    pub fn from_path(path: &Path) -> Option<Self> {
        let meta = fs::symlink_metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let dur = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
        Some(Self {
            mtime_secs: dur.as_secs(),
            mtime_nanos: dur.subsec_nanos(),
            len: meta.len(),
        })
    }
}

/// Snapshot of Git ref metadata mtimes for fast unchanged-repository detection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepoFingerprint {
    pub gitdir: PathBuf,
    pub commondir: PathBuf,
    pub branch: Option<String>,
    pub is_reftable: bool,
    pub head_stamp: Option<FileStamp>,
    pub branch_ref_stamp: Option<FileStamp>,
    pub packed_refs_stamp: Option<FileStamp>,
    pub remote_ref_stamp: Option<FileStamp>,
    pub remote_dir_stamp: Option<FileStamp>,
    pub fetch_head_stamp: Option<FileStamp>,
    pub reftable_tables_stamp: Option<FileStamp>,
}

/// Resolve `(gitdir, commondir)` for either a standard `.git` directory or a `.git` file
/// (used by linked worktrees and submodules) without spawning any subprocesses.
pub fn resolve_git_dirs(repo_path: &Path) -> Option<(PathBuf, PathBuf)> {
    let dot_git = repo_path.join(".git");
    let meta = fs::symlink_metadata(&dot_git).ok()?;
    let gitdir = if meta.is_dir() {
        dot_git
    } else if meta.is_file() {
        let content = fs::read_to_string(&dot_git).ok()?;
        let raw = content
            .lines()
            .find_map(|line| line.trim().strip_prefix("gitdir:"))?
            .trim();
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() {
            candidate
        } else {
            repo_path.join(candidate)
        }
    } else {
        return None;
    };

    let commondir_file = gitdir.join("commondir");
    let commondir = if commondir_file.is_file() {
        let raw = fs::read_to_string(&commondir_file).ok()?;
        let trimmed = raw.trim();
        let candidate = PathBuf::from(trimmed);
        if candidate.is_absolute() {
            candidate
        } else {
            gitdir.join(candidate)
        }
    } else {
        gitdir.clone()
    };

    Some((gitdir, commondir))
}

fn read_head_ref(gitdir: &Path) -> Option<String> {
    let head = fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let line = head.lines().next()?.trim();
    line.strip_prefix("ref:")
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(ToString::to_string)
}

impl RepoFingerprint {
    /// Capture ref-state file mtimes for `repo_path` via pure stat calls.
    pub fn capture(repo_path: &Path, default_branch: &str) -> Option<Self> {
        let (gitdir, commondir) = resolve_git_dirs(repo_path)?;
        let head_ref = read_head_ref(&gitdir);
        let branch = head_ref
            .as_deref()
            .and_then(|r| r.strip_prefix("refs/heads/"))
            .filter(|b| *b != ".invalid")
            .map(ToString::to_string);

        let reftable_tables = commondir.join("reftable").join("tables.list");
        let is_reftable = commondir.join("reftable").is_dir()
            || gitdir.join("reftable").is_dir()
            || head_ref.as_deref() == Some("refs/heads/.invalid");

        let branch_ref_stamp = head_ref
            .as_deref()
            .filter(|_| !is_reftable)
            .and_then(|r| FileStamp::from_path(&commondir.join(r)));

        let remote_dir = commondir.join("refs").join("remotes").join("origin");
        let remote_ref_path = remote_dir.join(default_branch);

        Some(Self {
            head_stamp: FileStamp::from_path(&gitdir.join("HEAD")),
            branch_ref_stamp,
            packed_refs_stamp: FileStamp::from_path(&commondir.join("packed-refs")),
            remote_ref_stamp: FileStamp::from_path(&remote_ref_path),
            remote_dir_stamp: FileStamp::from_path(&remote_dir),
            fetch_head_stamp: FileStamp::from_path(&gitdir.join("FETCH_HEAD")),
            reftable_tables_stamp: if is_reftable {
                FileStamp::from_path(&reftable_tables)
                    .or_else(|| FileStamp::from_path(&gitdir.join("reftable").join("tables.list")))
            } else {
                None
            },
            gitdir,
            commondir,
            branch,
            is_reftable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> (bool, String) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git installed");
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        )
    }

    #[test]
    fn fingerprint_hits_when_unchanged_and_misses_on_commit_not_worktree_edit() {
        let dir = crate::tests_fixture_temp_dir("fp-loose");
        let (ok, _) = git(&dir, &["init", "-b", "main"]);
        assert!(ok);
        git(&dir, &["config", "user.email", "t@example.invalid"]);
        git(&dir, &["config", "user.name", "Tester"]);
        fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "-m", "c1"]);

        let fp1 = RepoFingerprint::capture(&dir, "main").expect("fingerprint");
        assert!(!fp1.is_reftable);
        assert_eq!(fp1.branch.as_deref(), Some("main"));

        // Worktree-only edit must NOT alter ref fingerprint.
        fs::write(dir.join("a.txt"), "dirty\n").unwrap();
        let fp_dirty = RepoFingerprint::capture(&dir, "main").unwrap();
        assert_eq!(fp1, fp_dirty);

        // New commit MUST invalidate fingerprint.
        std::thread::sleep(std::time::Duration::from_millis(10));
        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "-m", "c2"]);
        let fp2 = RepoFingerprint::capture(&dir, "main").unwrap();
        assert_ne!(fp1, fp2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_supports_git_file_worktree() {
        let root = crate::tests_fixture_temp_dir("fp-worktree");
        let main_repo = root.join("main");
        let wt_repo = root.join("wt");
        fs::create_dir_all(&main_repo).unwrap();
        git(&main_repo, &["init", "-b", "main"]);
        git(&main_repo, &["config", "user.email", "t@example.invalid"]);
        git(&main_repo, &["config", "user.name", "Tester"]);
        fs::write(main_repo.join("f.txt"), "1\n").unwrap();
        git(&main_repo, &["add", "f.txt"]);
        git(&main_repo, &["commit", "-m", "init"]);
        let (ok, _) = git(
            &main_repo,
            &["worktree", "add", "-b", "feat", wt_repo.to_str().unwrap()],
        );
        assert!(ok);

        assert!(wt_repo.join(".git").is_file(), "worktree uses .git file");
        let fp1 = RepoFingerprint::capture(&wt_repo, "main").expect("worktree fingerprint");
        assert_eq!(fp1.branch.as_deref(), Some("feat"));
        assert_eq!(fp1, RepoFingerprint::capture(&wt_repo, "main").unwrap());

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(wt_repo.join("f.txt"), "2\n").unwrap();
        git(&wt_repo, &["add", "f.txt"]);
        git(&wt_repo, &["commit", "-m", "wt commit"]);
        let fp2 = RepoFingerprint::capture(&wt_repo, "main").unwrap();
        assert_ne!(fp1, fp2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_supports_reftable_storage() {
        let dir = crate::tests_fixture_temp_dir("fp-reftable");
        let (ok, _) = git(&dir, &["init", "--ref-format=reftable", "-b", "main"]);
        if !ok {
            // Skip cleanly on older git binaries without reftable support.
            let _ = fs::remove_dir_all(&dir);
            return;
        }
        git(&dir, &["config", "user.email", "t@example.invalid"]);
        git(&dir, &["config", "user.name", "Tester"]);
        fs::write(dir.join("r.txt"), "1\n").unwrap();
        git(&dir, &["add", "r.txt"]);
        git(&dir, &["commit", "-m", "init"]);

        let fp1 = RepoFingerprint::capture(&dir, "main").expect("reftable fingerprint");
        assert!(fp1.is_reftable);
        assert!(fp1.reftable_tables_stamp.is_some());
        assert_eq!(fp1, RepoFingerprint::capture(&dir, "main").unwrap());

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(dir.join("r.txt"), "2\n").unwrap();
        git(&dir, &["add", "r.txt"]);
        git(&dir, &["commit", "-m", "second"]);
        let fp2 = RepoFingerprint::capture(&dir, "main").unwrap();
        assert_ne!(fp1, fp2);

        let _ = fs::remove_dir_all(&dir);
    }
}
