//! Bounded concurrency for blocking background work.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const IO_WORKERS: usize = 8;

/// Map over items with a bounded number of worker threads while preserving input order.
pub fn map_bounded<T, R>(
    items: &[T],
    max_workers: usize,
    operation: impl Fn(&T) -> R + Sync,
) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    if items.is_empty() {
        return Vec::new();
    }

    let worker_count = max_workers.max(1).min(items.len());
    let next = AtomicUsize::new(0);
    let results = Mutex::new(
        std::iter::repeat_with(|| None)
            .take(items.len())
            .collect::<Vec<Option<R>>>(),
    );
    let trace_on = crate::modules::perf_trace::is_enabled();
    let batch_start = trace_on.then(std::time::Instant::now);
    let spans = trace_on.then(|| Mutex::new(Vec::with_capacity(items.len())));

    std::thread::scope(|scope| {
        for worker_id in 0..worker_count {
            let op = &operation;
            let next = &next;
            let results = &results;
            let spans = &spans;
            scope.spawn(move || {
                let mut local_spans = Vec::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let start_ns = batch_start.map_or(0, |b| b.elapsed().as_nanos() as u64);
                    let queue_wait_ns = start_ns;
                    let result = op(item);
                    let end_ns = batch_start.map_or(0, |b| b.elapsed().as_nanos() as u64);
                    if trace_on {
                        local_spans.push(crate::modules::perf_trace::WorkerSpan {
                            worker_id,
                            item_index: index,
                            queue_wait_ns,
                            start_ns,
                            end_ns,
                        });
                    }
                    if let Ok(mut results) = results.lock() {
                        results[index] = Some(result);
                    }
                }
                if let Some(spans) = spans
                    && let Ok(mut guard) = spans.lock()
                {
                    guard.extend(local_spans);
                }
            });
        }
    });

    if let Some(spans) = spans
        && let Ok(guard) = spans.into_inner()
    {
        crate::modules::perf_trace::store_worker_spans(guard);
    }

    results
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .into_iter()
        .map(|result| result.expect("worker must fill every result slot"))
        .collect()
}

/// Map over items bounded by both global workers and a per-key (e.g. per-host) active cap.
pub fn map_bounded_per_host<T, R>(
    items: &[T],
    max_workers: usize,
    per_host_cap: usize,
    host_of: impl Fn(&T) -> String + Sync,
    operation: impl Fn(&T) -> R + Sync,
) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    if items.is_empty() {
        return Vec::new();
    }

    let worker_count = max_workers.max(1).min(items.len());
    let host_cap = per_host_cap.max(1);
    let state = Mutex::new((
        vec![false; items.len()],
        std::collections::HashMap::<String, usize>::new(),
        0usize,
    ));
    let cvar = std::sync::Condvar::new();
    let hosts: Vec<String> = items.iter().map(&host_of).collect();
    let results = Mutex::new(
        std::iter::repeat_with(|| None)
            .take(items.len())
            .collect::<Vec<Option<R>>>(),
    );

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let picked = {
                        let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
                        loop {
                            let (taken, active_by_host, done_count) = &mut *guard;
                            if *done_count >= items.len() {
                                return;
                            }
                            let next_idx =
                                taken.iter().enumerate().find_map(|(idx, &was_taken)| {
                                    if was_taken {
                                        return None;
                                    }
                                    let active =
                                        active_by_host.get(&hosts[idx]).copied().unwrap_or(0);
                                    (active < host_cap).then_some(idx)
                                });
                            if let Some(idx) = next_idx {
                                taken[idx] = true;
                                *done_count += 1;
                                *active_by_host.entry(hosts[idx].clone()).or_insert(0) += 1;
                                break idx;
                            }
                            if taken.iter().all(|&t| t) {
                                return;
                            }
                            guard = cvar.wait(guard).unwrap_or_else(|p| p.into_inner());
                        }
                    };

                    let res = operation(&items[picked]);

                    if let Ok(mut out) = results.lock() {
                        out[picked] = Some(res);
                    }
                    {
                        let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
                        if let Some(count) = guard.1.get_mut(&hosts[picked]) {
                            *count = count.saturating_sub(1);
                        }
                        cvar.notify_all();
                    }
                }
            });
        }
    });

    results
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .into_iter()
        .map(|result| result.expect("worker must fill every result slot"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[test]
    fn preserves_order_and_runs_concurrently() {
        let items = (0..24).collect::<Vec<_>>();
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);

        let results = map_bounded(&items, 4, |item| {
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(current, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(10));
            active.fetch_sub(1, Ordering::SeqCst);
            item * 2
        });

        assert_eq!(results, (0..24).map(|item| item * 2).collect::<Vec<_>>());
        assert!(peak.load(Ordering::SeqCst) > 1);
        assert!(peak.load(Ordering::SeqCst) <= 4);
    }

    #[test]
    fn empty_input_does_not_spawn_workers() {
        let results = map_bounded(&[] as &[u8], 8, |_| unreachable!());
        assert!(results.is_empty());
    }

    #[test]
    fn per_host_cap_limits_same_host_while_allowing_other_hosts() {
        let items = vec![
            ("github.com", 0usize),
            ("github.com", 1),
            ("github.com", 2),
            ("github.com", 3),
            ("gitlab.com", 4),
        ];
        let active_gh = AtomicUsize::new(0);
        let max_gh = AtomicUsize::new(0);
        let active_total = AtomicUsize::new(0);
        let max_total = AtomicUsize::new(0);

        let out = map_bounded_per_host(
            &items,
            3,
            2,
            |(host, _)| (*host).to_string(),
            |(host, idx)| {
                let total = active_total.fetch_add(1, Ordering::Relaxed) + 1;
                max_total.fetch_max(total, Ordering::Relaxed);
                if *host == "github.com" {
                    let gh = active_gh.fetch_add(1, Ordering::Relaxed) + 1;
                    max_gh.fetch_max(gh, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(30));
                if *host == "github.com" {
                    active_gh.fetch_sub(1, Ordering::Relaxed);
                }
                active_total.fetch_sub(1, Ordering::Relaxed);
                *idx
            },
        );
        assert_eq!(out, vec![0, 1, 2, 3, 4]);
        assert!(
            max_gh.load(Ordering::Relaxed) <= 2,
            "same-host concurrency exceeded cap: {}",
            max_gh.load(Ordering::Relaxed)
        );
        assert_eq!(max_total.load(Ordering::Relaxed), 3);
    }
}
