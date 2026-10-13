//! Typed start functions copy borrowed inputs on the caller thread. Only owned
//! operations reach worker threads, so waits cannot exhaust a foreign worker pool.
use super::*;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, TryRecvError};

type Completion = Result<OperationResult>;

thread_local! {
    static START: RefCell<Option<Option<Receiver<Completion>>>> = const { RefCell::new(None) };
}

/// An owned asynchronous call. Poll until a result arrives, then free the task.
/// Freeing a pending task discards its eventual result without canceling the call.
pub struct TuiTask {
    receiver: Option<Receiver<Completion>>,
    immediate: *mut TuiResult,
}

impl Drop for TuiTask {
    fn drop(&mut self) {
        unsafe { output::free(self.immediate) };
    }
}

pub(super) fn execute(work: impl FnOnce() -> Completion + Send + 'static) -> *mut TuiResult {
    if !START.with(|slot| slot.borrow().is_some()) {
        return boundary(work);
    }
    let (sender, receiver) = mpsc::channel();
    START.with(|slot| *slot.borrow_mut() = Some(Some(receiver)));
    match std::thread::Builder::new()
        .name("tui-test-call".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|payload| {
                let message = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic payload");
                Err(TuiTestError::internal(format!(
                    "native engine adapter panicked: {message}"
                )))
            });
            // Dropping the receiver abandons the result, which remains fully owned.
            drop(sender.send(result));
        }) {
        Ok(_) => std::ptr::null_mut(),
        Err(error) => {
            START.with(|slot| *slot.borrow_mut() = Some(None));
            output::error(TuiTestError::internal(format!(
                "native worker failed: {error}"
            )))
        }
    }
}

pub(super) fn start(call: impl FnOnce() -> *mut TuiResult) -> *mut TuiTask {
    START.with(|slot| *slot.borrow_mut() = Some(None));
    let immediate = call();
    let receiver = START.with(|slot| slot.borrow_mut().take().flatten());
    Box::into_raw(Box::new(TuiTask {
        receiver,
        immediate,
    }))
}

#[no_mangle]
/// Returns NULL while pending. Transfers ownership of a completed result once.
///
/// # Safety
/// task must be a live task returned by a start function, used by one thread.
pub unsafe extern "C" fn tui_task_poll(task: *mut TuiTask) -> *mut TuiResult {
    let task = unsafe { &mut *task };
    if !task.immediate.is_null() {
        return std::mem::replace(&mut task.immediate, std::ptr::null_mut());
    }
    let Some(receiver) = &task.receiver else {
        return std::ptr::null_mut();
    };
    match receiver.try_recv() {
        Ok(completion) => {
            task.receiver = None;
            match completion {
                Ok(value) => output::operation(value),
                Err(error) => output::error(error),
            }
        }
        Err(TryRecvError::Empty) => std::ptr::null_mut(),
        Err(TryRecvError::Disconnected) => {
            task.receiver = None;
            output::error(TuiTestError::internal("native worker disconnected"))
        }
    }
}

#[no_mangle]
/// # Safety
/// task must be NULL or a live task, freed exactly once.
pub unsafe extern "C" fn tui_task_free(task: *mut TuiTask) {
    if !task.is_null() {
        drop(unsafe { Box::from_raw(task) });
    }
}

#[no_mangle]
/// Exercise panic containment without terminating the host process.
pub extern "C" fn tui_panic_probe_start() -> *mut TuiTask {
    start(|| execute(|| panic!("intentional native panic probe")))
}
