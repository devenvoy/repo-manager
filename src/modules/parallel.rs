//! Bounded concurrency for blocking background work.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) const IO_WORKERS: usize = 8;

/// Map over items with a bounded number of worker threads while preserving input order.
pub(crate) fn map_bounded<T, R>(
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

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result = operation(item);
                    if let Ok(mut results) = results.lock() {
                        results[index] = Some(result);
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
}
