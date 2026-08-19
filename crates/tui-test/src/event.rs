use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::api::BellEvent;

const MAX_BELL_EVENTS: usize = 1024;

#[derive(Clone)]
pub(crate) struct BellTracker {
    inner: Arc<BellTrackerInner>,
}

struct BellTrackerInner {
    start: Instant,
    published_sequence: AtomicU64,
    state: Mutex<BellState>,
}

#[derive(Default)]
struct BellState {
    count: u64,
    sequence: u64,
    events: VecDeque<BellEvent>,
}

pub(crate) struct BellSnapshot {
    pub count: u64,
    pub events: Vec<BellEvent>,
}

impl Default for BellTracker {
    fn default() -> Self {
        Self::new(Instant::now())
    }
}

impl BellTracker {
    pub fn new(start: Instant) -> Self {
        Self {
            inner: Arc::new(BellTrackerInner {
                start,
                published_sequence: AtomicU64::new(0),
                state: Mutex::new(BellState::default()),
            }),
        }
    }

    pub fn count(&self) -> u64 {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .count
    }

    pub fn sequence(&self) -> u64 {
        self.inner.published_sequence.load(Ordering::Acquire)
    }

    pub fn snapshot(&self) -> BellSnapshot {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        BellSnapshot {
            count: state.count,
            events: state.events.iter().copied().collect(),
        }
    }

    pub fn ring(&self) {
        let elapsed_ms = self
            .inner
            .start
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let sequence = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.count = state.count.saturating_add(1);
            state.sequence = state.sequence.wrapping_add(1);
            if state.events.len() == MAX_BELL_EVENTS {
                state.events.pop_front();
            }
            let sequence = state.sequence;
            state.events.push_back(BellEvent {
                sequence,
                elapsed_ms,
            });
            sequence
        };
        self.inner
            .published_sequence
            .store(sequence, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn bell_events_include_sequence_and_elapsed_time() {
        let bells = BellTracker::new(Instant::now() - Duration::from_millis(10));
        bells.ring();
        bells.ring();

        let snapshot = bells.snapshot();
        assert_eq!(snapshot.count, 2);
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(snapshot.events[0].sequence, 1);
        assert_eq!(snapshot.events[1].sequence, 2);
        assert!(snapshot.events[0].elapsed_ms >= 10);
        assert!(snapshot.events[1].elapsed_ms >= snapshot.events[0].elapsed_ms);
    }

    #[test]
    fn bell_event_history_is_bounded_without_losing_the_count() {
        let bells = BellTracker::default();
        for _ in 0..=MAX_BELL_EVENTS {
            bells.ring();
        }

        let snapshot = bells.snapshot();
        assert_eq!(snapshot.count, (MAX_BELL_EVENTS + 1) as u64);
        assert_eq!(snapshot.events.len(), MAX_BELL_EVENTS);
        assert_eq!(snapshot.events[0].sequence, 2);
        assert_eq!(
            snapshot.events[MAX_BELL_EVENTS - 1].sequence,
            (MAX_BELL_EVENTS + 1) as u64
        );
    }
}
