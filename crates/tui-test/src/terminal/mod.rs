pub mod alacritty;
pub mod backend;
pub mod cell;
#[cfg(test)]
pub mod conformance;
#[cfg(windows)]
pub mod conpty;
#[cfg(any(windows, test))]
mod conpty_assets;
pub mod emu;
#[cfg(feature = "ghostty")]
pub mod ghostty;
pub mod integration;
pub mod locator;
mod program_status;
pub mod pty;
pub(crate) mod reports;
#[cfg(feature = "rio")]
pub mod rio;
#[cfg(feature = "xtermjs")]
pub mod xtermjs;
