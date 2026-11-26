//! App-owned per-project job registry, RAII active-job guards, and structured results.

use crate::controllers::git_controller::GitActivity;
use crate::modules::repo_cache::{
    CachedRepoRecord, ProbeRepoIssue, RepoCache, StoredCommit, StoredReleaseAsset,
};
use crate::repository_git::{RepositoryStatus, is_incomplete_clone};
use crate::views::project_repository_status::{LanguageBreakdownState, RepositoryPresentation};
use crate::workspace::WorkspaceProject;
use crate::workspace_sources::RepoKey;
use release_desk_git::CloneStatus;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Explicit toast severity derived from structured outcomes rather than string keywords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastSeverity {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusToast {
    pub severity: ToastSeverity,
    pub text: String,
}

impl StatusToast {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            severity: ToastSeverity::Info,
            text: text.into(),
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            severity: ToastSeverity::Success,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            severity: ToastSeverity::Warning,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            severity: ToastSeverity::Error,
            text: text.into(),
        }
    }
}

/// Per-repository clone state machine surfaced in the Repositories UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoClonePhase {
    Queued,
    Cloning { percent: Option<u8>, detail: String },
    Verifying,
    Done,
    Failed(String),
    Cancelled,
}

impl RepoClonePhase {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Queued | Self::Cloning { .. } | Self::Verifying)
    }
}

/// Structured outcome of a batch or single-repository clone job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloneBatchSummary {
    pub total: usize,
    pub succeeded: Vec<RepoKey>,
    pub failed: Vec<(RepoKey, String)>,
    pub cancelled: Vec<RepoKey>,
}

impl CloneBatchSummary {
    pub fn to_toast(&self) -> StatusToast {
        if self.total == 0 {
            return StatusToast::info("No selected repositories need cloning.");
        }
        if self.failed.is_empty() && self.cancelled.is_empty() {
            let count = self.succeeded.len();
            let noun = if count == 1 {
                "repository"
            } else {
                "repositories"
            };
            return StatusToast::success(format!("Cloned {count} {noun}."));
        }
        if self.succeeded.is_empty() && !self.failed.is_empty() {
            let preview = self
                .failed
                .iter()
                .take(2)
                .map(|(k, err)| format!("{}: {err}", k.name))
                .collect::<Vec<_>>()
                .join(" · ");
            return StatusToast::error(format!(
                "Failed to clone {} of {} repositories: {preview}",
                self.failed.len(),
                self.total
            ));
        }
        if self.succeeded.is_empty() && self.failed.is_empty() {
            return StatusToast::warning(format!(
                "Cancelled {} repository clone(s).",
                self.cancelled.len()
            ));
        }
        let preview = self
            .failed
            .iter()
            .take(2)
            .map(|(k, err)| format!("{}: {err}", k.name))
            .collect::<Vec<_>>()
            .join(" · ");
        StatusToast::warning(format!(
            "Cloned {} of {}; {} failed, {} cancelled. {preview}",
            self.succeeded.len(),
            self.total,
            self.failed.len(),
            self.cancelled.len()
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectJobKind {
    Inspect,
    Check,
    Clone,
}

/// Which workflow produced a [`RepoUpdate::ReplaceRecord`], used to enforce lazy-field
/// preservation guarantees and invariant checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoUpdateKind {
    Inspect,
    Check,
    Fetch,
    Pull,
    Revalidate,
}

/// Every mutation to per-repository state passes through [`RepoUpdate`] and the single
/// coordinator [`ReleaseDesk::apply_repo_update`].
#[derive(Clone, Debug)]
pub enum RepoUpdate {
    ReplaceRecord {
        record: Box<CachedRepoRecord>,
        kind: RepoUpdateKind,
    },
    SetLazyDetail {
        commits: Vec<StoredCommit>,
        has_untracked: Option<(bool, u64)>,
    },
    SetIcon(Option<PathBuf>),
    ClearStaleIndex {
        refreshed_at: u64,
    },
    ClearStaleLocks,
    ApplyProbe {
        remote_sha: Option<String>,
        failed_reason: Option<String>,
        issue: Option<ProbeRepoIssue>,
        latest_release: Option<(String, Option<String>, Vec<StoredReleaseAsset>)>,
        probed_at: u64,
    },
    MarkOffline,
    Remove,
}

impl RepoUpdate {
    pub fn replace(record: CachedRepoRecord, kind: RepoUpdateKind) -> Self {
        Self::ReplaceRecord {
            record: Box::new(record),
            kind,
        }
    }
}

/// Authoritative per-repository state entry; `status` and `presentation` are private cached
/// projections derived atomically from `record` whenever `version` increments.
#[derive(Clone, Debug)]
pub struct RepoStateEntry {
    record: CachedRepoRecord,
    version: u64,
    status: RepositoryStatus,
    presentation: RepositoryPresentation,
}

impl RepoStateEntry {
    pub fn from_record(record: CachedRepoRecord, version: u64) -> Self {
        let status = record.to_repository_status();
        let presentation = record.to_presentation();
        let entry = Self {
            record,
            version,
            status,
            presentation,
        };
        #[cfg(debug_assertions)]
        assert_entry_invariants(&entry, None);
        entry
    }

    pub fn record(&self) -> &CachedRepoRecord {
        &self.record
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn status(&self) -> &RepositoryStatus {
        &self.status
    }

    pub fn presentation(&self) -> &RepositoryPresentation {
        &self.presentation
    }

    pub fn is_incomplete(&self) -> bool {
        is_incomplete_clone(&self.status)
    }

    pub fn is_cloned(&self) -> bool {
        self.record.destination.is_git_repo() && !self.is_incomplete()
    }

    pub fn is_missing_clone(&self) -> bool {
        !self.is_incomplete()
            && (self.record.destination == release_desk_git::DestinationKind::Missing
                || self.record.effective_clone_status() == CloneStatus::NotCloned)
    }

    pub fn can_pull(&self) -> bool {
        crate::repository_git::can_pull(&self.status) && self.record.stale_locks.is_empty()
    }

    pub fn row_summary(&self) -> RepoRowSummary {
        let is_incomplete = self.is_incomplete();
        let is_uncloned = !is_incomplete
            && (self.record.destination.is_clonable()
                || self.record.effective_clone_status() == CloneStatus::NotCloned);
        let eff = self.record.effective_clone_status();
        let is_checked = self.record.last_probed_at.is_some()
            || self.record.last_fetched_at.is_some()
            || self.status.fetched;
        let has_update = self.record.has_probed_update() || eff == CloneStatus::Behind;
        let has_failed = self.record.probe_failed_reason.is_some()
            || self.status.state.is_err()
            || eff == CloneStatus::Error;
        RepoRowSummary {
            is_uncloned,
            can_pull: self.can_pull(),
            is_checked,
            has_update,
            has_failed,
        }
    }
}

/// Row-level boolean flags summed identically by the header and row list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepoRowSummary {
    pub is_uncloned: bool,
    pub can_pull: bool,
    pub is_checked: bool,
    pub has_update: bool,
    pub has_failed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProjectSummaryCounts {
    pub uncloned_count: usize,
    pub pull_count: usize,
    pub checked_count: usize,
    pub updates_count: usize,
    pub failed_count: usize,
}

impl ProjectSummaryCounts {
    pub fn from_repos<'a>(
        keys: impl IntoIterator<Item = &'a RepoKey>,
        repos: &HashMap<RepoKey, RepoStateEntry>,
    ) -> Self {
        let mut counts = Self::default();
        for key in keys {
            if let Some(entry) = repos.get(key) {
                let row = entry.row_summary();
                if row.is_uncloned {
                    counts.uncloned_count += 1;
                }
                if row.can_pull {
                    counts.pull_count += 1;
                }
                if row.is_checked {
                    counts.checked_count += 1;
                }
                if row.has_update {
                    counts.updates_count += 1;
                }
                if row.has_failed {
                    counts.failed_count += 1;
                }
            }
        }
        counts
    }
}

/// Assert D3 per-entry state invariants:
/// 1) A `GitRepo` checkout never renders/derives as missing or incomplete.
/// 2) `ahead_count`/`behind_count` and `status` agree.
/// 3) Lazy commits are not dropped across check, fetch, probe, or same-HEAD revalidation.
pub fn assert_entry_invariants(
    entry: &RepoStateEntry,
    preserved_commits_check: Option<&[StoredCommit]>,
) {
    let rec = &entry.record;
    if rec.destination.is_git_repo() {
        assert!(
            entry.presentation.present,
            "GitRepo {} must have presentation.present == true",
            rec.key.name
        );
        assert!(
            entry.status.destination.is_git_repo(),
            "GitRepo {} must have status.destination.is_git_repo()",
            rec.key.name
        );
        assert_ne!(
            rec.effective_clone_status(),
            CloneStatus::NotCloned,
            "GitRepo {} must never resolve as NotCloned",
            rec.key.name
        );
        assert!(
            entry.is_cloned() && !entry.is_incomplete() && !entry.is_missing_clone(),
            "GitRepo {} must classify as is_cloned=true and not incomplete/missing",
            rec.key.name
        );
    }
    match rec.status {
        CloneStatus::Current => {
            if rec.ahead_count.is_some() || rec.behind_count.is_some() {
                assert_eq!(
                    (rec.ahead_count, rec.behind_count),
                    (Some(0), Some(0)),
                    "Current repo {} counts must be (0, 0), got ({:?}, {:?})",
                    rec.key.name,
                    rec.ahead_count,
                    rec.behind_count
                );
            }
        }
        CloneStatus::Behind => {
            if rec.ahead_count.is_some() || rec.behind_count.is_some() {
                assert!(
                    rec.behind_count.is_some_and(|b| b > 0) && rec.ahead_count == Some(0),
                    "Behind repo {} counts must be (0, >0), got ({:?}, {:?})",
                    rec.key.name,
                    rec.ahead_count,
                    rec.behind_count
                );
            }
        }
        CloneStatus::Ahead => {
            if rec.ahead_count.is_some() || rec.behind_count.is_some() {
                assert!(
                    rec.ahead_count.is_some_and(|a| a > 0) && rec.behind_count == Some(0),
                    "Ahead repo {} counts must be (>0, 0), got ({:?}, {:?})",
                    rec.key.name,
                    rec.ahead_count,
                    rec.behind_count
                );
            }
        }
        CloneStatus::Diverged => {
            if rec.ahead_count.is_some() || rec.behind_count.is_some() {
                assert!(
                    rec.ahead_count.is_some_and(|a| a > 0)
                        && rec.behind_count.is_some_and(|b| b > 0),
                    "Diverged repo {} counts must be (>0, >0), got ({:?}, {:?})",
                    rec.key.name,
                    rec.ahead_count,
                    rec.behind_count
                );
            }
        }
        CloneStatus::NotCloned | CloneStatus::Error => {
            assert_eq!(
                (rec.ahead_count, rec.behind_count),
                (None, None),
                "NotCloned/Error repo {} must not carry stale ahead/behind counts",
                rec.key.name
            );
        }
        CloneStatus::Dirty | CloneStatus::Detached | CloneStatus::OtherBranch => {}
    }
    if let Some(prev_commits) = preserved_commits_check
        && !prev_commits.is_empty()
    {
        assert!(
            !rec.recent_commits.is_empty(),
            "Check/Fetch/Probe/Revalidate dropped commits for {}",
            rec.key.name
        );
    }
}

/// Assert all D3 project-level invariants (entries, header counts == sum of rows, SQLite == memory).
pub fn assert_repo_state_invariants(
    project: &WorkspaceProject,
    runtime: &ProjectRuntimeState,
    cache: Option<&RepoCache>,
) {
    let selected_keys = project.selected_repo_keys();
    let expected_counts = ProjectSummaryCounts::from_repos(selected_keys.iter(), runtime.repos());
    let mut row_uncloned = 0usize;
    let mut row_pull = 0usize;
    let mut row_checked = 0usize;
    let mut row_updates = 0usize;
    let mut row_failed = 0usize;
    for key in &selected_keys {
        if let Some(entry) = runtime.repo(key) {
            assert_entry_invariants(entry, None);
            let s = entry.row_summary();
            row_uncloned += usize::from(s.is_uncloned);
            row_pull += usize::from(s.can_pull);
            row_checked += usize::from(s.is_checked);
            row_updates += usize::from(s.has_update);
            row_failed += usize::from(s.has_failed);
        }
    }
    assert_eq!(
        expected_counts,
        ProjectSummaryCounts {
            uncloned_count: row_uncloned,
            pull_count: row_pull,
            checked_count: row_checked,
            updates_count: row_updates,
            failed_count: row_failed,
        }
    );
    if let Some(cache) = cache {
        let persisted = cache.load_project_repos(project.id);
        assert_eq!(
            persisted.len(),
            runtime.repos().len(),
            "persisted record count ({}) != in-memory count ({})",
            persisted.len(),
            runtime.repos().len()
        );
        for (key, entry) in runtime.repos() {
            assert_eq!(
                persisted.get(key),
                Some(entry.record()),
                "persisted record for {} differs from in-memory record",
                key.name
            );
        }
    }
}

/// Per-project runtime state owned by the application root, surviving modal close,
/// tab switches, and project switches.
#[derive(Clone, Debug)]
pub struct ProjectRuntimeState {
    repos: HashMap<RepoKey, RepoStateEntry>,
    pub cache_hydrated: bool,
    pub offline: bool,
    pub last_checked_at: Option<u64>,
    pub pending: HashSet<RepoKey>,
    pub selected_repository: Option<RepoKey>,
    pub languages_expanded: HashSet<RepoKey>,
    pub language_breakdowns: HashMap<RepoKey, LanguageBreakdownState>,
    pub clone_states: HashMap<RepoKey, RepoClonePhase>,
    pub active_inspect: Option<u64>,
    pub active_check: Option<u64>,
    pub active_clone: Option<u64>,
    pub active_pull_queue: bool,
    pub activity: Option<GitActivity>,
    pub check_completed: usize,
    pub check_total: usize,
    pub probe_completed: usize,
    pub probe_total: usize,
    pub probe_failed: usize,
    pub cancel: Arc<AtomicBool>,
}

impl Default for ProjectRuntimeState {
    fn default() -> Self {
        Self {
            repos: HashMap::new(),
            cache_hydrated: false,
            offline: false,
            last_checked_at: None,
            pending: HashSet::new(),
            selected_repository: None,
            languages_expanded: HashSet::new(),
            language_breakdowns: HashMap::new(),
            clone_states: HashMap::new(),
            active_inspect: None,
            active_check: None,
            active_clone: None,
            active_pull_queue: false,
            activity: None,
            check_completed: 0,
            check_total: 0,
            probe_completed: 0,
            probe_total: 0,
            probe_failed: 0,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl ProjectRuntimeState {
    pub fn repos(&self) -> &HashMap<RepoKey, RepoStateEntry> {
        &self.repos
    }

    pub fn repo(&self, key: &RepoKey) -> Option<&RepoStateEntry> {
        self.repos.get(key)
    }

    pub fn repo_records_cloned(&self) -> HashMap<RepoKey, CachedRepoRecord> {
        self.repos
            .iter()
            .map(|(k, e)| (k.clone(), e.record().clone()))
            .collect()
    }

    pub fn is_inspect_or_check_busy(&self) -> bool {
        self.active_inspect.is_some() || self.active_check.is_some() || self.active_pull_queue
    }

    pub fn is_clone_busy(&self) -> bool {
        self.active_clone.is_some()
    }

    pub fn is_any_busy(&self) -> bool {
        self.is_inspect_or_check_busy() || self.is_clone_busy()
    }

    fn recompute_probe_counters(&mut self) {
        self.probe_total = self.repos.len();
        let mut ok_count = 0usize;
        let mut failed_count = 0usize;
        for entry in self.repos.values() {
            let rec = entry.record();
            if rec.probe_failed_reason.is_some()
                || rec.effective_clone_status() == CloneStatus::Error
            {
                failed_count += 1;
            } else {
                ok_count += 1;
            }
        }
        self.probe_completed = ok_count;
        self.probe_failed = failed_count;
    }

    /// Populate unified repository entries from persisted SQLite records with zero git/FS work.
    pub fn hydrate_from_records(&mut self, records: HashMap<RepoKey, CachedRepoRecord>) {
        let mut latest_probe: Option<u64> = self.last_checked_at;
        self.repos.clear();
        for (key, rec) in records {
            if let Some(t) = rec.last_probed_at.or(rec.last_inspected_at) {
                latest_probe = Some(latest_probe.map_or(t, |prev| prev.max(t)));
            }
            self.repos.insert(key, RepoStateEntry::from_record(rec, 1));
        }
        self.recompute_probe_counters();
        self.last_checked_at = latest_probe;
        self.cache_hydrated = true;
    }

    /// Apply one [`RepoUpdate`] atomically in memory, preserving lazy fields unless explicitly
    /// replaced/invalidated and bumping the per-repo version. Returns `Some(rec)` to persist,
    /// or `None` when removed or no-op.
    pub(crate) fn apply_repo_update_in_memory(
        &mut self,
        key: &RepoKey,
        update: RepoUpdate,
    ) -> Option<CachedRepoRecord> {
        let prev_entry = self.repos.get(key).cloned();
        let prev_version = prev_entry.as_ref().map_or(0, RepoStateEntry::version);
        let prev_rec = prev_entry.as_ref().map(RepoStateEntry::record);

        match update {
            RepoUpdate::Remove => {
                self.repos.remove(key);
                self.recompute_probe_counters();
                None
            }
            RepoUpdate::ReplaceRecord { mut record, kind } => {
                #[cfg(debug_assertions)]
                let preserved_commits_check: Option<Vec<StoredCommit>> =
                    prev_rec.and_then(|prev| {
                        (!prev.recent_commits.is_empty()
                            && matches!(
                                kind,
                                RepoUpdateKind::Check
                                    | RepoUpdateKind::Fetch
                                    | RepoUpdateKind::Revalidate
                            )
                            && record.destination.is_git_repo()
                            && record.head_sha == prev.head_sha)
                            .then(|| prev.recent_commits.clone())
                    });
                if let Some(prev) = prev_rec {
                    if record.recent_commits.is_empty()
                        && record.destination.is_git_repo()
                        && (record.head_sha == prev.head_sha
                            || !matches!(kind, RepoUpdateKind::Pull))
                    {
                        record.recent_commits = prev.recent_commits.clone();
                    }
                    if record.icon_path.is_none() {
                        record.icon_path = prev.icon_path.clone();
                    }
                    if record.latest_release_tag.is_none() {
                        record.latest_release_tag = prev.latest_release_tag.clone();
                        record.latest_release_date = prev.latest_release_date.clone();
                        record.latest_release_assets = prev.latest_release_assets.clone();
                    }
                    if record.installed_version.is_none() {
                        record.installed_version = prev.installed_version.clone();
                    }
                    if record.has_untracked.is_none() && record.destination.is_git_repo() {
                        record.has_untracked = prev.has_untracked;
                        record.has_untracked_at = prev.has_untracked_at;
                    }
                    if matches!(kind, RepoUpdateKind::Inspect | RepoUpdateKind::Revalidate)
                        && prev.last_probed_at.is_some()
                        && prev.last_probed_at >= record.last_probed_at
                    {
                        record.probed_remote_sha = prev.probed_remote_sha.clone();
                        record.last_probed_at = prev.last_probed_at;
                        record.probe_failed_reason = prev.probe_failed_reason.clone();
                        record.probe_issue = prev.probe_issue.clone();
                    }
                }
                let record = *record;
                let entry =
                    RepoStateEntry::from_record(record.clone(), prev_version.saturating_add(1));
                #[cfg(debug_assertions)]
                assert_entry_invariants(&entry, preserved_commits_check.as_deref());
                self.repos.insert(key.clone(), entry);
                self.recompute_probe_counters();
                Some(record)
            }
            RepoUpdate::SetLazyDetail {
                commits,
                has_untracked,
            } => {
                let prev = prev_rec?;
                let mut record = prev.clone();
                if record.destination.is_git_repo() {
                    record.recent_commits = commits;
                    if let Some((u, ts)) = has_untracked {
                        record.has_untracked = Some(u);
                        record.has_untracked_at = Some(ts);
                    }
                }
                let entry =
                    RepoStateEntry::from_record(record.clone(), prev_version.saturating_add(1));
                self.repos.insert(key.clone(), entry);
                self.recompute_probe_counters();
                Some(record)
            }
            RepoUpdate::SetIcon(icon_path) => {
                let prev = prev_rec?;
                if prev.icon_path == icon_path {
                    return None;
                }
                let mut record = prev.clone();
                record.icon_path = icon_path;
                let entry =
                    RepoStateEntry::from_record(record.clone(), prev_version.saturating_add(1));
                self.repos.insert(key.clone(), entry);
                Some(record)
            }
            RepoUpdate::ClearStaleIndex { refreshed_at } => {
                let prev = prev_rec?;
                let mut record = prev.clone();
                record.stale_index_suspected = false;
                record.status_slow_streak = 0;
                record.last_index_refresh_at = Some(refreshed_at);
                let entry =
                    RepoStateEntry::from_record(record.clone(), prev_version.saturating_add(1));
                self.repos.insert(key.clone(), entry);
                self.recompute_probe_counters();
                Some(record)
            }
            RepoUpdate::ClearStaleLocks => {
                let prev = prev_rec?;
                let mut record = prev.clone();
                record.stale_locks.clear();
                let entry =
                    RepoStateEntry::from_record(record.clone(), prev_version.saturating_add(1));
                self.repos.insert(key.clone(), entry);
                self.recompute_probe_counters();
                Some(record)
            }
            RepoUpdate::ApplyProbe {
                remote_sha,
                failed_reason,
                issue,
                latest_release,
                probed_at,
            } => {
                let prev = prev_rec?;
                #[cfg(debug_assertions)]
                let preserved = prev.recent_commits.clone();
                let mut record = prev.clone();
                record.last_probed_at = Some(probed_at);
                record.probe_issue = issue;
                if let Some(sha) = remote_sha {
                    record.probed_remote_sha = Some(sha);
                    record.probe_failed_reason = None;
                } else if let Some(err) = failed_reason {
                    record.probe_failed_reason = Some(err);
                }
                if let Some((tag, date, assets)) = latest_release {
                    record.latest_release_tag = Some(tag);
                    record.latest_release_date = date;
                    record.latest_release_assets = assets;
                }
                let entry =
                    RepoStateEntry::from_record(record.clone(), prev_version.saturating_add(1));
                #[cfg(debug_assertions)]
                assert_entry_invariants(&entry, Some(&preserved));
                self.repos.insert(key.clone(), entry);
                self.recompute_probe_counters();
                Some(record)
            }
            RepoUpdate::MarkOffline => {
                let prev = prev_rec?;
                let mut record = prev.clone();
                record.probe_issue = Some(ProbeRepoIssue::Offline);
                let entry =
                    RepoStateEntry::from_record(record.clone(), prev_version.saturating_add(1));
                self.repos.insert(key.clone(), entry);
                self.recompute_probe_counters();
                Some(record)
            }
        }
    }

    /// Retain only keys still present in `valid_keys`, preserving cached state for untouched repos.
    pub fn retain_valid_keys(&mut self, valid_keys: &HashSet<RepoKey>) {
        self.repos.retain(|k, _| valid_keys.contains(k));
        self.recompute_probe_counters();
        self.pending.retain(|k| valid_keys.contains(k));
        self.languages_expanded.retain(|k| valid_keys.contains(k));
        self.language_breakdowns
            .retain(|k, _| valid_keys.contains(k));
        self.clone_states
            .retain(|k, v| valid_keys.contains(k) || v.is_active());
        if self
            .selected_repository
            .as_ref()
            .is_some_and(|k| !valid_keys.contains(k))
        {
            self.selected_repository = None;
        }
    }
}

/// App-wide job registry keyed by `project_id`.
#[derive(Debug, Default)]
pub struct JobRegistry {
    next_job_id: u64,
    projects: HashMap<u128, ProjectRuntimeState>,
}

impl JobRegistry {
    pub fn project(&self, project_id: u128) -> Option<&ProjectRuntimeState> {
        self.projects.get(&project_id)
    }

    pub fn project_mut(&mut self, project_id: u128) -> &mut ProjectRuntimeState {
        self.projects.entry(project_id).or_default()
    }

    pub fn remove_project(&mut self, project_id: u128) {
        if let Some(state) = self.projects.remove(&project_id) {
            state.cancel.store(true, Ordering::Release);
        }
    }

    pub fn cancel_project_jobs(&mut self, project_id: u128) {
        if let Some(state) = self.projects.get_mut(&project_id) {
            state.cancel.store(true, Ordering::Release);
            state.cancel = Arc::new(AtomicBool::new(false));
        }
    }

    pub fn begin_job(
        &mut self,
        project_id: u128,
        kind: ProjectJobKind,
    ) -> Option<(u64, Arc<AtomicBool>)> {
        let busy = self
            .projects
            .get(&project_id)
            .is_some_and(|state| match kind {
                ProjectJobKind::Inspect | ProjectJobKind::Check => state.is_inspect_or_check_busy(),
                ProjectJobKind::Clone => state.active_clone.is_some(),
            });
        if busy {
            return None;
        }
        self.next_job_id = self.next_job_id.wrapping_add(1).max(1);
        let job_id = self.next_job_id;
        let state = self.project_mut(project_id);
        match kind {
            ProjectJobKind::Inspect => state.active_inspect = Some(job_id),
            ProjectJobKind::Check => state.active_check = Some(job_id),
            ProjectJobKind::Clone => state.active_clone = Some(job_id),
        }
        Some((job_id, state.cancel.clone()))
    }

    /// Unconditionally release a job slot when its work finishes, panics, or disconnects.
    /// Never gated on `workspace_revision` or `selected_project_id`.
    pub fn finish_job(&mut self, project_id: u128, job_id: u64, kind: ProjectJobKind) {
        let Some(state) = self.projects.get_mut(&project_id) else {
            return;
        };
        match kind {
            ProjectJobKind::Inspect if state.active_inspect == Some(job_id) => {
                state.active_inspect = None;
                state.pending.clear();
                state.check_total = 0;
                state.check_completed = 0;
                if state.active_check.is_none() && !state.active_pull_queue {
                    state.activity = None;
                }
            }
            ProjectJobKind::Check if state.active_check == Some(job_id) => {
                state.active_check = None;
                state.pending.clear();
                state.check_total = 0;
                state.check_completed = 0;
                if state.active_inspect.is_none() && !state.active_pull_queue {
                    state.activity = None;
                }
            }
            ProjectJobKind::Clone if state.active_clone == Some(job_id) => {
                state.active_clone = None;
            }
            _ => {}
        }
    }
}

/// RAII guard for background threads ensuring an abort notification is always sent even on panic.
pub(crate) struct WorkerDropGuard<F: FnOnce()> {
    on_abort: Option<F>,
}

impl<F: FnOnce()> WorkerDropGuard<F> {
    pub fn new(on_abort: F) -> Self {
        Self {
            on_abort: Some(on_abort),
        }
    }

    pub fn disarm(&mut self) {
        self.on_abort = None;
    }
}

impl<F: FnOnce()> Drop for WorkerDropGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.on_abort.take() {
            f();
        }
    }
}
