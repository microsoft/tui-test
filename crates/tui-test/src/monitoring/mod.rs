//! Opt-in monitoring of terminals owned by the current process.

mod bridge;
mod facade;
pub mod host;
pub mod ipc;
mod lifecycle;
pub mod protocol;

#[doc(hidden)]
pub mod ansi;
#[doc(hidden)]
pub mod input;
#[doc(hidden)]
pub mod render;

pub use bridge::{
    begin_wait, begin_wait_for_target_with_options, begin_wait_with_options, cancel_target,
    cancel_wait, clear_sessions, invalidate_replaced, register, unregister, wait, wait_target,
    Metadata,
};
pub(crate) use bridge::{closed, prepare_close, prepare_replace};
pub use facade::{Monitor, Options, WaitPolicy};
pub use lifecycle::Outcome;
