//! Sharing work over many items among the computer's processors.

/// The most threads one piece of work is shared among.
const MAX_THREADS: usize = 8;

/// How many threads to share `count` items among, when each thread should have at least `least_each`: 1 when
/// there are too few to be worth sharing.
pub fn threads_for(count: usize, least_each: usize) -> usize {
    let available = std::thread::available_parallelism().map_or(1, |n| n.get());
    available.min(MAX_THREADS).min(count / least_each.max(1)).max(1)
}

/// Do `work` on the items, in runs shared among threads when there are at least `least_each` items for each of
/// two or more, and give what the runs made in the items' order. `work` is handed one run and where it starts
/// among all the items. A run whose thread failed gives nothing.
pub fn map_runs<T: Sync, R: Send>(items: &[T], least_each: usize, work: impl Fn(usize, &[T]) -> Vec<R> + Sync) -> Vec<R> {
    let threads = threads_for(items.len(), least_each);
    if threads < 2 {
        return work(0, items);
    }
    let size = items.len().div_ceil(threads);
    let work = &work;
    std::thread::scope(|scope| {
        let runs: Vec<_> = items.chunks(size).enumerate().map(|(index, run)| scope.spawn(move || work(index * size, run))).collect();
        runs.into_iter().flat_map(|run| run.join().unwrap_or_default()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_come_back_in_the_items_order_however_they_are_shared() {
        let items: Vec<usize> = (0..10_000).collect();
        for least_each in [1, 100, 5000, 20_000] {
            let doubled = map_runs(&items, least_each, |start, run| {
                assert_eq!(run[0], start, "a run knows where it starts");
                run.iter().map(|i| i * 2).collect()
            });
            assert_eq!(doubled, items.iter().map(|i| i * 2).collect::<Vec<_>>(), "at least {least_each} each");
        }
        assert!(map_runs(&[] as &[usize], 1, |_, run| run.to_vec()).is_empty());
    }
}
