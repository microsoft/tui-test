use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[derive(Clone, Copy, Default)]
pub(crate) struct Statistics {
    pub allocations: usize,
    pub peak: usize,
    live: isize,
}

thread_local! {
    static ACTIVE: Cell<Option<Statistics>> = const { Cell::new(None) };
}

struct Allocator;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn record(added: usize, removed: usize, allocation: bool) {
    let _ = ACTIVE.try_with(|active| {
        if let Some(mut stats) = active.get() {
            stats.allocations += usize::from(allocation);
            stats.live += added as isize - removed as isize;
            stats.peak = stats.peak.max(stats.live.max(0) as usize);
            active.set(Some(stats));
        }
    });
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record(layout.size(), 0, true);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record(layout.size(), 0, true);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record(0, layout.size(), false);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            record(size, layout.size(), true);
        }
        result
    }
}

pub(crate) fn measure<T>(run: impl FnOnce() -> T) -> (T, Statistics) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|active| active.set(None));
        }
    }
    ACTIVE.with(|active| assert!(active.replace(Some(Statistics::default())).is_none()));
    let reset = Reset;
    let result = run();
    let statistics = ACTIVE.with(|active| active.get().unwrap());
    drop(reset);
    (result, statistics)
}
