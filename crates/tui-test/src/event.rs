use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Default)]
pub(crate) struct BellTracker {
    count: Arc<AtomicU64>,
    sequence: Arc<AtomicU64>,
}

impl BellTracker {
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }

    pub fn ring(&self) {
        let _ = self
            .count
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                Some(count.saturating_add(1))
            });
        self.sequence.fetch_add(1, Ordering::Relaxed);
    }
}
