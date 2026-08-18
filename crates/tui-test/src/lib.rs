pub mod api;
pub mod assert;
pub mod config;
pub mod engine;
pub mod input;
pub mod logger;
pub mod profile;
#[allow(dead_code)] // The session lifecycle consumes the capture API next in the stack.
pub mod record;
pub mod render;
pub mod runtime;
pub mod shell;
pub mod terminal;
pub mod trace;

mod session;

pub use api::*;
pub use engine::Engine;
pub use runtime::{global_registry, Session, SessionHandle, SessionRegistry};
pub use terminal::backend::Backend;
