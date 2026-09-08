use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::engine::MonitorPty;

#[derive(Default)]
pub(crate) struct Viewports {
    epoch: u64,
    entries: BTreeMap<u64, Entry>,
    resizing: Arc<Mutex<()>>,
}

struct Entry {
    target: Option<MonitorPty>,
    size: (u16, u16),
    interactive: bool,
    updated: u64,
}

pub(crate) struct Viewport {
    views: Arc<Mutex<Viewports>>,
    id: u64,
    applied: Mutex<Option<(MonitorPty, u64)>>,
    resizing: Arc<Mutex<()>>,
}

impl Viewport {
    pub(crate) fn new(
        views: Arc<Mutex<Viewports>>,
        target: Option<MonitorPty>,
        size: (u16, u16),
        interactive: bool,
    ) -> Self {
        let mut state = views
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.epoch += 1;
        let id = state.epoch;
        state.entries.insert(
            id,
            Entry {
                target,
                size,
                interactive,
                updated: id,
            },
        );
        let resizing = state.resizing.clone();
        drop(state);
        Self {
            views,
            id,
            applied: Mutex::new(None),
            resizing,
        }
    }

    pub(crate) fn update(&self, size: (u16, u16)) {
        let mut state = self
            .views
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.epoch += 1;
        let updated = state.epoch;
        let entry = state
            .entries
            .get_mut(&self.id)
            .expect("live viewport registration");
        entry.size = size;
        entry.updated = updated;
    }

    pub(crate) fn apply(
        &self,
        target: &MonitorPty,
        resize: impl FnOnce((u16, u16)) -> Result<(), crate::TuiTestError>,
    ) -> Result<(), crate::TuiTestError> {
        let _resizing = self
            .resizing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((size, epoch)) = self.pending(target) {
            resize(size)?;
            *self
                .applied
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((target.clone(), epoch));
        }
        Ok(())
    }

    fn pending(&self, target: &MonitorPty) -> Option<((u16, u16), u64)> {
        let state = self
            .views
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (&selected, entry) = state
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry
                    .target
                    .as_ref()
                    .is_none_or(|expected| Arc::ptr_eq(expected, target))
            })
            .max_by_key(|(_, entry)| (entry.interactive, entry.updated))?;
        if selected != self.id {
            return None;
        }
        let applied = self
            .applied
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if applied
            .as_ref()
            .is_some_and(|(pty, epoch)| Arc::ptr_eq(pty, target) && *epoch == state.epoch)
        {
            return None;
        }
        Some((entry.size, state.epoch))
    }
}

impl Drop for Viewport {
    fn drop(&mut self) {
        let mut state = self
            .views
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.entries.remove(&self.id);
        state.epoch += 1;
    }
}
