//! Lightweight opt-in timing trace for presentation building and parallel worker timelines.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed) || release_desk_git::perf::is_enabled()
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    release_desk_git::perf::set_enabled(enabled);
}

#[derive(Clone, Debug, Default)]
pub struct PresentationTimings {
    pub resolve_path_ns: u64,
    pub icon_resolve_ns: u64,
    pub cached_release_ns: u64,
    pub present_check_ns: u64,
    pub recent_commits_ns: u64,
    pub total_ns: u64,
}

#[derive(Clone, Debug)]
pub struct WorkerSpan {
    pub worker_id: usize,
    pub item_index: usize,
    pub queue_wait_ns: u64,
    pub start_ns: u64,
    pub end_ns: u64,
}

thread_local! {
    static LAST_PRESENTATION: RefCell<Option<PresentationTimings>> = const { RefCell::new(None) };
    static LAST_WORKER_SPANS: RefCell<Vec<WorkerSpan>> = const { RefCell::new(Vec::new()) };
}

#[inline]
pub fn start() -> Option<Instant> {
    is_enabled().then(Instant::now)
}

#[inline]
pub fn elapsed_ns(started: Option<Instant>) -> u64 {
    started.map_or(0, |t| t.elapsed().as_nanos() as u64)
}

pub fn store_presentation(timings: PresentationTimings) {
    LAST_PRESENTATION.with(|cell| {
        *cell.borrow_mut() = Some(timings);
    });
}

pub fn take_last_presentation() -> Option<PresentationTimings> {
    LAST_PRESENTATION.with(|cell| cell.borrow_mut().take())
}

pub fn store_worker_spans(spans: Vec<WorkerSpan>) {
    LAST_WORKER_SPANS.with(|cell| {
        *cell.borrow_mut() = spans;
    });
}

pub fn take_worker_spans() -> Vec<WorkerSpan> {
    LAST_WORKER_SPANS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}
