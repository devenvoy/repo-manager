//! Lightweight, opt-in timing instrumentation for Git operations.
//!
//! Inert unless explicitly enabled via [`set_enabled`] or `RELEASE_DESK_PERF=1`.

use std::cell::RefCell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

static ENV_INIT: OnceLock<bool> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static TOTAL_SUBPROCESSES: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static THREAD_SUBPROCESSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[inline]
pub(crate) fn record_spawn() {
    TOTAL_SUBPROCESSES.fetch_add(1, Ordering::Relaxed);
    THREAD_SUBPROCESSES.with(|c| c.set(c.get() + 1));
}

pub fn total_subprocesses() -> usize {
    TOTAL_SUBPROCESSES.load(Ordering::SeqCst)
}

pub fn thread_subprocesses() -> usize {
    THREAD_SUBPROCESSES.with(std::cell::Cell::get)
}

/// Returns whether Git timing instrumentation is currently active.
#[inline]
pub fn is_enabled() -> bool {
    if ENABLED.load(Ordering::Relaxed) {
        return true;
    }
    *ENV_INIT.get_or_init(|| {
        std::env::var_os("RELEASE_DESK_PERF").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

/// Explicitly enable or disable Git timing instrumentation for benchmarking.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

#[derive(Clone, Debug, Default)]
pub struct InspectTimings {
    pub path_check_ns: u64,
    pub worktree_verify_ns: u64,
    pub head_read_ns: u64,
    pub remote_ref_read_ns: u64,
    pub branch_check_ns: u64,
    pub status_porcelain_ns: u64,
    pub ancestry_ns: u64,
    pub total_ns: u64,
    pub subprocesses: u32,
    pub subprocess_ns: u64,
}

#[derive(Clone, Debug, Default)]
pub struct FetchTimings {
    pub pre_inspect_ns: u64,
    pub origin_verify_ns: u64,
    pub fetch_origin_ns: u64,
    pub post_inspect_ns: u64,
    pub total_ns: u64,
    pub subprocesses: u32,
    pub subprocess_ns: u64,
}

#[derive(Clone, Debug)]
pub struct SubprocessSample {
    pub operation: &'static str,
    pub duration_ns: u64,
    pub exit_code: Option<i32>,
}

thread_local! {
    static CURRENT_SUBPROCESSES: RefCell<(u32, u64, Vec<SubprocessSample>)> =
        const { RefCell::new((0, 0, Vec::new())) };
    static LAST_INSPECT: RefCell<Option<InspectTimings>> = const { RefCell::new(None) };
    static LAST_FETCH: RefCell<Option<FetchTimings>> = const { RefCell::new(None) };
}

#[inline]
pub(crate) fn start() -> Option<Instant> {
    is_enabled().then(Instant::now)
}

#[inline]
pub(crate) fn elapsed_ns(started: Option<Instant>) -> u64 {
    started.map_or(0, |t| t.elapsed().as_nanos() as u64)
}

pub(crate) fn reset_subprocess_counter() -> (u32, u64) {
    CURRENT_SUBPROCESSES.with(|cell| {
        let state = cell.borrow();
        (state.0, state.1)
    })
}

pub(crate) fn record_subprocess(
    operation: &'static str,
    started: Option<Instant>,
    exit_code: Option<i32>,
) {
    let Some(t) = started else {
        return;
    };
    let ns = t.elapsed().as_nanos() as u64;
    CURRENT_SUBPROCESSES.with(|cell| {
        let mut state = cell.borrow_mut();
        state.0 += 1;
        state.1 += ns;
        if state.2.len() < 4096 {
            state.2.push(SubprocessSample {
                operation,
                duration_ns: ns,
                exit_code,
            });
        }
    });
}

pub(crate) fn subprocess_delta(before: (u32, u64)) -> (u32, u64) {
    CURRENT_SUBPROCESSES.with(|cell| {
        let state = cell.borrow();
        (
            state.0.saturating_sub(before.0),
            state.1.saturating_sub(before.1),
        )
    })
}

pub(crate) fn store_inspect(timings: InspectTimings) {
    LAST_INSPECT.with(|cell| {
        *cell.borrow_mut() = Some(timings);
    });
}

pub(crate) fn store_fetch(timings: FetchTimings) {
    LAST_FETCH.with(|cell| {
        *cell.borrow_mut() = Some(timings);
    });
}

/// Take the most recent [`InspectTimings`] recorded on the current thread.
pub fn take_last_inspect() -> Option<InspectTimings> {
    LAST_INSPECT.with(|cell| cell.borrow_mut().take())
}

/// Take the most recent [`FetchTimings`] recorded on the current thread.
pub fn take_last_fetch() -> Option<FetchTimings> {
    LAST_FETCH.with(|cell| cell.borrow_mut().take())
}

/// Take and clear recorded [`SubprocessSample`] entries on the current thread.
pub fn take_subprocess_samples() -> Vec<SubprocessSample> {
    CURRENT_SUBPROCESSES.with(|cell| {
        let mut state = cell.borrow_mut();
        state.0 = 0;
        state.1 = 0;
        std::mem::take(&mut state.2)
    })
}
