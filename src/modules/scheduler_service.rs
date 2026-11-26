//! Project-scoped scheduler, per-volume AIMD local concurrency limiter (`st_dev`),
//! slowest-first ordering, and trigger coalescing.

use crate::modules::repo_cache::{RepoCache, VolumeLimitRecord, now_unix_secs};
use crate::workspace_sources::RepoKey;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const INITIAL_VOLUME_CONCURRENCY: usize = 2;
pub const MAX_LOCAL_SSD_CONCURRENCY: usize = 4;
pub const MAX_NETWORK_FS_CONCURRENCY: usize = 2;
pub const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub const DEFAULT_FOCUS_COOLDOWN: Duration = Duration::from_secs(60);
pub const LIMIT_DECAY_SECS: u64 = 3600;

/// Abstract monotonic + wall clock for testable scheduler cooldowns and intervals.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn unix_secs(&self) -> u64;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn unix_secs(&self) -> u64 {
        now_unix_secs()
    }
}

/// Deterministic controllable clock for scheduler and AIMD decay tests.
#[derive(Debug)]
pub struct FakeClock {
    state: Mutex<(Instant, u64)>,
}

impl FakeClock {
    pub fn new(unix_secs: u64) -> Self {
        Self {
            state: Mutex::new((Instant::now(), unix_secs)),
        }
    }

    pub fn advance(&self, delta: Duration) {
        let mut g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        g.0 += delta;
        g.1 = g.1.saturating_add(delta.as_secs());
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).0
    }

    fn unix_secs(&self) -> u64 {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).1
    }
}

/// Decays persisted volume concurrency limits back toward [`INITIAL_VOLUME_CONCURRENCY`]
/// when stored samples age out (hourly +1 step toward initial, reset after 24h).
pub fn decay_persisted_limit(saved: &VolumeLimitRecord, now_secs: u64, max_cap: usize) -> usize {
    let clamped = saved.learned_concurrency.clamp(1, max_cap);
    let age = now_secs.saturating_sub(saved.updated_at);
    if age >= LIMIT_DECAY_SECS * 24 {
        return INITIAL_VOLUME_CONCURRENCY.clamp(1, max_cap);
    }
    let steps = (age / LIMIT_DECAY_SECS) as usize;
    if steps == 0 || clamped >= INITIAL_VOLUME_CONCURRENCY {
        clamped
    } else {
        (clamped + steps)
            .min(INITIAL_VOLUME_CONCURRENCY)
            .min(max_cap)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TriggerKind {
    Startup,
    Focus,
    Interval,
    ManualCheck,
}

/// Detect whether `path` sits on a network mount (`smbfs`, `nfs`, `afpfs`, `cifs`) without unsafe code.
pub fn is_network_filesystem(path: &Path) -> bool {
    static MOUNTS: OnceLock<Vec<(String, String)>> = OnceLock::new();
    let mounts = MOUNTS.get_or_init(|| {
        let Ok(out) = std::process::Command::new("/sbin/mount").output() else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parsed = Vec::new();
        for line in text.lines() {
            // Format: "<dev> on <mountpoint> (<fstype>, ...)"
            if let Some((_, rhs)) = line.split_once(" on ")
                && let Some((mnt, attrs)) = rhs.rsplit_once(" (")
            {
                let fstype = attrs
                    .split([',', ')'])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                parsed.push((mnt.to_string(), fstype));
            }
        }
        parsed.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
        parsed
    });

    let path_str = path.to_string_lossy();
    mounts
        .iter()
        .find(|(mnt, _)| {
            path_str == *mnt || (mnt == "/" || path_str.starts_with(&format!("{mnt}/")))
        })
        .is_some_and(|(_, fstype)| matches!(fstype.as_str(), "smbfs" | "nfs" | "afpfs" | "cifs"))
}

/// Resolve OS `st_dev` device identifier for `path` (walking ancestors if missing).
pub fn volume_dev_for_path(path: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let mut cur: Option<&Path> = Some(path);
        while let Some(p) = cur {
            if let Ok(meta) = std::fs::symlink_metadata(p) {
                return meta.dev();
            }
            cur = p.parent();
        }
    }
    0
}

#[derive(Clone, Debug)]
struct VolumeState {
    limit: usize,
    max_cap: usize,
    ewma_latency_ms: f64,
    fast_streak: usize,
    spike_streak: usize,
}

/// Per-volume AIMD concurrency controller keyed by OS `st_dev` with per-repo EWMA baseline
/// normalization so large repositories do not trigger spurious congestion drops.
pub struct VolumeConcurrencyLimiter {
    volumes: Mutex<HashMap<u64, VolumeState>>,
    repo_baselines: Mutex<HashMap<PathBuf, f64>>,
    clock: Arc<dyn Clock>,
}

impl Default for VolumeConcurrencyLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl VolumeConcurrencyLimiter {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }

    pub fn with_clock(clock: Arc<dyn Clock>) -> Self {
        Self {
            volumes: Mutex::new(HashMap::new()),
            repo_baselines: Mutex::new(HashMap::new()),
            clock,
        }
    }

    pub fn current_limit(&self, dev: u64, is_network: bool, cache: Option<&RepoCache>) -> usize {
        let max_cap = if is_network {
            MAX_NETWORK_FS_CONCURRENCY
        } else {
            MAX_LOCAL_SSD_CONCURRENCY
        };
        let now_secs = self.clock.unix_secs();
        let mut guard = self.volumes.lock().unwrap_or_else(|p| p.into_inner());
        let state = guard.entry(dev).or_insert_with(|| {
            if let Some(saved) = cache.and_then(|c| c.load_volume_limit(dev)) {
                VolumeState {
                    limit: decay_persisted_limit(&saved, now_secs, max_cap),
                    max_cap,
                    ewma_latency_ms: saved.ewma_latency_ms,
                    fast_streak: 0,
                    spike_streak: 0,
                }
            } else {
                VolumeState {
                    limit: INITIAL_VOLUME_CONCURRENCY.clamp(1, max_cap),
                    max_cap,
                    ewma_latency_ms: 0.0,
                    fast_streak: 0,
                    spike_streak: 0,
                }
            }
        });
        state.limit
    }

    /// Update AIMD state for `dev` after one operation completes, without per-repo normalization.
    pub fn record_sample(
        &self,
        dev: u64,
        is_network: bool,
        duration_ms: f64,
        timed_out: bool,
        cache: Option<&RepoCache>,
    ) -> usize {
        self.record_repo_sample(dev, None, is_network, duration_ms, timed_out, cache)
    }

    /// Update AIMD state normalized against `repo_path`'s own EWMA latency baseline so a large
    /// repository running near its normal speed does not lower volume concurrency, and one-off spikes
    /// do not pin the limit at 1.
    pub fn record_repo_sample(
        &self,
        dev: u64,
        repo_path: Option<&Path>,
        is_network: bool,
        duration_ms: f64,
        timed_out: bool,
        cache: Option<&RepoCache>,
    ) -> usize {
        let max_cap = if is_network {
            MAX_NETWORK_FS_CONCURRENCY
        } else {
            MAX_LOCAL_SSD_CONCURRENCY
        };

        let ratio = if let Some(path) = repo_path {
            let mut baselines = self
                .repo_baselines
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let entry = baselines
                .entry(path.to_path_buf())
                .or_insert_with(|| duration_ms.max(40.0));
            let base = (*entry).max(40.0);
            let r = duration_ms / base;
            if !timed_out {
                // Smoothly track repository's own baseline (slower alpha upward on spikes).
                let alpha = if r > 2.5 { 0.08 } else { 0.25 };
                *entry = alpha * duration_ms + (1.0 - alpha) * base;
            }
            r
        } else {
            duration_ms / 600.0
        };

        let (new_limit, ewma) = {
            let mut guard = self.volumes.lock().unwrap_or_else(|p| p.into_inner());
            let state = guard.entry(dev).or_insert_with(|| VolumeState {
                limit: INITIAL_VOLUME_CONCURRENCY.clamp(1, max_cap),
                max_cap,
                ewma_latency_ms: duration_ms,
                fast_streak: 0,
                spike_streak: 0,
            });
            state.ewma_latency_ms = if state.ewma_latency_ms <= 0.0 {
                duration_ms
            } else {
                0.35 * duration_ms + 0.65 * state.ewma_latency_ms
            };

            let elevated_spike = !timed_out && ratio > 2.5 && duration_ms > 500.0;
            if timed_out {
                state.limit = (state.limit / 2).max(1);
                state.fast_streak = 0;
                state.spike_streak = 0;
            } else if elevated_spike {
                state.spike_streak += 1;
                state.fast_streak = 0;
                // Require 2 elevated-ratio samples to filter transient one-off OS/GC jitter.
                if state.spike_streak >= 2 {
                    state.limit = (state.limit / 2).max(1);
                    state.spike_streak = 0;
                }
            } else {
                state.spike_streak = 0;
                let can_advance = if state.limit < INITIAL_VOLUME_CONCURRENCY {
                    ratio <= 1.6 || duration_ms < 250.0
                } else {
                    // Only scale above INITIAL_VOLUME_CONCURRENCY (2 -> 3..4) on genuinely fast
                    // volumes (SSDs), never because a first-run HDD sample has ratio == 1.0.
                    duration_ms <= 250.0 && state.ewma_latency_ms <= 350.0
                };
                if can_advance {
                    state.fast_streak += 1;
                    let recovery_threshold = if state.limit < INITIAL_VOLUME_CONCURRENCY {
                        2
                    } else {
                        4
                    };
                    if state.fast_streak >= recovery_threshold {
                        state.limit = (state.limit + 1).min(state.max_cap);
                        state.fast_streak = 0;
                    }
                } else {
                    state.fast_streak = 0;
                }
            }
            (state.limit, state.ewma_latency_ms)
        };
        if let Some(cache) = cache {
            let _ = cache.save_volume_limit(&VolumeLimitRecord {
                volume_dev: dev,
                learned_concurrency: new_limit,
                ewma_latency_ms: ewma,
                updated_at: self.clock.unix_secs(),
            });
        }
        new_limit
    }

    /// Execute `operation` across `items` dynamically bounded by the volume's current AIMD limit.
    pub fn run_on_volume<T, R>(
        &self,
        volume_path: &Path,
        cache: Option<&RepoCache>,
        items: &[T],
        operation: impl Fn(&T) -> (R, bool) + Sync,
    ) -> Vec<R>
    where
        T: Sync,
        R: Send,
    {
        self.run_on_volume_with_paths(volume_path, cache, items, |_| None, operation)
    }

    /// Execute `operation` with per-item repository paths so AIMD normalizes latency per repo.
    pub fn run_on_volume_with_paths<T, R>(
        &self,
        volume_path: &Path,
        cache: Option<&RepoCache>,
        items: &[T],
        repo_path_of: impl Fn(&T) -> Option<&Path> + Sync,
        operation: impl Fn(&T) -> (R, bool) + Sync,
    ) -> Vec<R>
    where
        T: Sync,
        R: Send,
    {
        if items.is_empty() {
            return Vec::new();
        }
        let dev = volume_dev_for_path(volume_path);
        let is_net = is_network_filesystem(volume_path);
        let initial = self.current_limit(dev, is_net, cache);
        let max_workers = MAX_LOCAL_SSD_CONCURRENCY.min(items.len()).max(1);

        let shared = Mutex::new((0usize, 0usize, initial));
        let cvar = Condvar::new();
        let results = Mutex::new(
            std::iter::repeat_with(|| None)
                .take(items.len())
                .collect::<Vec<Option<R>>>(),
        );

        std::thread::scope(|scope| {
            for _ in 0..max_workers {
                scope.spawn(|| {
                    loop {
                        let idx = {
                            let mut guard = shared.lock().unwrap_or_else(|p| p.into_inner());
                            loop {
                                let (next_idx, active, allowed) = *guard;
                                if next_idx >= items.len() {
                                    return;
                                }
                                if active < allowed.max(1) {
                                    guard.0 += 1;
                                    guard.1 += 1;
                                    break next_idx;
                                }
                                guard = cvar.wait(guard).unwrap_or_else(|p| p.into_inner());
                            }
                        };

                        let t0 = self.clock.now();
                        let (val, timed_out) = operation(&items[idx]);
                        let elapsed_ms =
                            self.clock.now().saturating_duration_since(t0).as_secs_f64() * 1000.0;
                        let updated_limit = self.record_repo_sample(
                            dev,
                            repo_path_of(&items[idx]),
                            is_net,
                            elapsed_ms,
                            timed_out,
                            cache,
                        );

                        if let Ok(mut r) = results.lock() {
                            r[idx] = Some(val);
                        }
                        {
                            let mut guard = shared.lock().unwrap_or_else(|p| p.into_inner());
                            guard.1 = guard.1.saturating_sub(1);
                            guard.2 = updated_limit;
                            cvar.notify_all();
                        }
                    }
                });
            }
        });

        results
            .into_inner()
            .unwrap_or_else(|p| p.into_inner())
            .into_iter()
            .map(|x| x.expect("worker filled slot"))
            .collect()
    }
}

/// Rank repositories for revalidation: selected/visible first, then oldest `last_probed_at`.
pub fn prioritize_probe_order(
    keys: &[RepoKey],
    selected: Option<&RepoKey>,
    visible: &HashSet<RepoKey>,
    last_probed_at: &HashMap<RepoKey, u64>,
) -> Vec<RepoKey> {
    let mut ordered = keys.to_vec();
    ordered.sort_by(|a, b| {
        let a_sel = selected == Some(a);
        let b_sel = selected == Some(b);
        b_sel
            .cmp(&a_sel)
            .then_with(|| visible.contains(b).cmp(&visible.contains(a)))
            .then_with(|| {
                let pa = last_probed_at.get(a).copied().unwrap_or(0);
                let pb = last_probed_at.get(b).copied().unwrap_or(0);
                pa.cmp(&pb)
            })
            .then_with(|| a.cmp(b))
    });
    ordered
}

/// Coalesce duplicate background trigger requests per project without overlapping in-flight runs.
pub struct TriggerCoalescer {
    in_flight: HashSet<u128>,
    pending: HashSet<(u128, TriggerKind)>,
    last_completed: HashMap<(u128, TriggerKind), Instant>,
    clock: Arc<dyn Clock>,
}

impl Default for TriggerCoalescer {
    fn default() -> Self {
        Self::new(Arc::new(SystemClock))
    }
}

impl std::fmt::Debug for TriggerCoalescer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriggerCoalescer")
            .field("in_flight", &self.in_flight)
            .field("pending", &self.pending)
            .finish()
    }
}

impl TriggerCoalescer {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            in_flight: HashSet::new(),
            pending: HashSet::new(),
            last_completed: HashMap::new(),
            clock,
        }
    }

    pub fn request(&mut self, project_id: u128, kind: TriggerKind, min_cooldown: Duration) -> bool {
        if self.in_flight.contains(&project_id) || self.pending.contains(&(project_id, kind)) {
            return false;
        }
        if kind != TriggerKind::ManualCheck
            && let Some(prev) = self.last_completed.get(&(project_id, kind))
            && self.clock.now().saturating_duration_since(*prev) < min_cooldown
        {
            return false;
        }
        self.in_flight.insert(project_id);
        self.pending.insert((project_id, kind));
        true
    }

    pub fn complete(&mut self, project_id: u128, kind: TriggerKind) {
        self.in_flight.remove(&project_id);
        self.pending.remove(&(project_id, kind));
        self.last_completed
            .insert((project_id, kind), self.clock.now());
    }
}

/// Order targets slowest/largest first so 1 slow repo doesn't become a tail straggler.
pub fn sort_slowest_first<T>(items: &mut [T], key_of: impl Fn(&T) -> PathBuf) {
    items.sort_by(|a, b| {
        let sa = std::fs::metadata(key_of(a).join(".git/index")).map_or(0, |m| m.len());
        let sb = std::fs::metadata(key_of(b).join(".git/index")).map_or(0, |m| m.len());
        sb.cmp(&sa)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aimd_limiter_drops_on_latency_spike_and_recovers_on_fast_runs() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("aimd.sqlite3");
        let cache = RepoCache::open_at(&db).unwrap();
        let limiter = VolumeConcurrencyLimiter::new();
        let dev = 42_000u64;

        // Starts at 2.
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), 2);

        // 4 fast completions -> additive increase to 3!
        for _ in 0..4 {
            limiter.record_sample(dev, false, 30.0, false, Some(&cache));
        }
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), 3);

        // Injected latency spike / timeout -> multiplicative decrease to 1!
        limiter.record_sample(dev, false, 2400.0, true, Some(&cache));
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), 1);

        // Learned limit persists in RepoCache across restart.
        let limiter2 = VolumeConcurrencyLimiter::new();
        assert_eq!(limiter2.current_limit(dev, false, Some(&cache)), 1);
    }

    #[test]
    fn prioritize_and_coalesce_scheduler_rules() {
        let k1 = RepoKey::new("github", "github.com", "o", "oldest");
        let k2 = RepoKey::new("github", "github.com", "o", "visible");
        let k3 = RepoKey::new("github", "github.com", "o", "selected");
        let probed = HashMap::from([(k1.clone(), 100), (k2.clone(), 500), (k3.clone(), 900)]);
        let visible = HashSet::from([k2.clone()]);
        let order = prioritize_probe_order(
            &[k1.clone(), k2.clone(), k3.clone()],
            Some(&k3),
            &visible,
            &probed,
        );
        assert_eq!(order, vec![k3, k2, k1]);

        let mut coalescer = TriggerCoalescer::default();
        assert!(coalescer.request(1, TriggerKind::Focus, Duration::from_secs(60)));
        assert!(!coalescer.request(1, TriggerKind::Focus, Duration::from_secs(60)));
        coalescer.complete(1, TriggerKind::Focus);
        // Cooldown blocks immediate re-focus trigger, while ManualCheck always runs.
        assert!(!coalescer.request(1, TriggerKind::Focus, Duration::from_secs(60)));
        assert!(coalescer.request(1, TriggerKind::ManualCheck, Duration::from_secs(60)));
    }

    #[test]
    fn c3_aimd_handles_transient_spike_large_repo_contention_and_decay() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("c3.sqlite3");
        let cache = RepoCache::open_at(&db).unwrap();
        let clock = Arc::new(FakeClock::new(10_000));
        let limiter = VolumeConcurrencyLimiter::with_clock(clock.clone());
        let dev = 99_001u64;
        let large_repo = dir.path().join("monorepo-large");
        let normal_repo = dir.path().join("small-repo");

        // Large repo consistently takes 2,400 ms on its own -> baseline adapts, never drops concurrency.
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), 2);
        for _ in 0..5 {
            let lim = limiter.record_repo_sample(
                dev,
                Some(&large_repo),
                false,
                2400.0,
                false,
                Some(&cache),
            );
            assert!(
                lim >= 2,
                "large repo alone must not reduce concurrency below 2"
            );
        }

        // One-off transient spike (timeout drops 2 -> 1), then recovers via additive increase!
        limiter.record_repo_sample(dev, Some(&normal_repo), false, 80.0, false, Some(&cache));
        limiter.record_repo_sample(dev, Some(&normal_repo), false, 5000.0, true, Some(&cache));
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), 1);
        for _ in 0..2 {
            limiter.record_repo_sample(dev, Some(&normal_repo), false, 85.0, false, Some(&cache));
        }
        assert_eq!(
            limiter.current_limit(dev, false, Some(&cache)),
            2,
            "one-off spike must not pin limit at 1 permanently"
        );

        // Single non-timeout spike does NOT drop; sustained contention (>= 2 elevated ratio samples) DOES lower it.
        let before = limiter.current_limit(dev, false, Some(&cache));
        limiter.record_repo_sample(dev, Some(&normal_repo), false, 900.0, false, Some(&cache));
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), before);
        limiter.record_repo_sample(dev, Some(&normal_repo), false, 950.0, false, Some(&cache));
        assert_eq!(limiter.current_limit(dev, false, Some(&cache)), 1);

        // Advance FakeClock 2 hours -> persisted limit=1 decays back toward initial=2.
        clock.advance(Duration::from_secs(2 * 3600));
        let limiter_after_idle = VolumeConcurrencyLimiter::with_clock(clock);
        assert_eq!(
            limiter_after_idle.current_limit(dev, false, Some(&cache)),
            2
        );
    }
}
