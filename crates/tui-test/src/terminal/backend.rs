//! Terminal emulator selection and construction.

use serde::{Deserialize, Serialize};

use crate::event::BellTracker;
use crate::profile::Profile;
use crate::terminal::alacritty::AlacrittyEmu;
use crate::terminal::emu::Emulator;

#[cfg(feature = "ghostty")]
use crate::terminal::ghostty::GhosttyEmu;
#[cfg(feature = "xtermjs")]
use crate::terminal::xtermjs::XtermJsEmu;

/// The terminal emulator a session uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Alacritty,
    #[cfg(feature = "ghostty")]
    Ghostty,
    #[cfg(feature = "xtermjs")]
    Xtermjs,
}

impl Backend {
    /// Every backend this build can construct.
    ///
    /// A slice rather than a fixed-size array so that each optional backend
    /// adds one line here instead of doubling the number of length-carrying
    /// definitions the next one has to spell out.
    pub const ALL: &'static [Self] = &[
        Self::Alacritty,
        #[cfg(feature = "ghostty")]
        Self::Ghostty,
        #[cfg(feature = "xtermjs")]
        Self::Xtermjs,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alacritty => "alacritty",
            #[cfg(feature = "ghostty")]
            Self::Ghostty => "ghostty",
            #[cfg(feature = "xtermjs")]
            Self::Xtermjs => "xtermjs",
        }
    }

    pub fn build(
        self,
        cols: u16,
        rows: u16,
        profile: &Profile,
    ) -> anyhow::Result<Box<dyn Emulator>> {
        match self {
            Self::Alacritty => Ok(Box::new(AlacrittyEmu::new(cols, rows, profile))),
            #[cfg(feature = "ghostty")]
            Self::Ghostty => Ok(Box::new(GhosttyEmu::new(cols, rows, profile)?)),
            #[cfg(feature = "xtermjs")]
            Self::Xtermjs => Ok(Box::new(XtermJsEmu::new(cols, rows, profile)?)),
        }
    }

    /// Like [`Self::build`], but wires up bell tracking where the backend
    /// supports it. Backends without native bell support simply won't
    /// report bell events.
    pub(crate) fn build_with_bells(
        self,
        cols: u16,
        rows: u16,
        profile: &Profile,
        bells: BellTracker,
    ) -> anyhow::Result<Box<dyn Emulator>> {
        match self {
            Self::Alacritty => Ok(Box::new(AlacrittyEmu::with_bell_tracker(
                cols, rows, profile, bells,
            ))),
            // Backends that do not report bells themselves are built the
            // ordinary way and simply never raise one.
            #[cfg(feature = "ghostty")]
            Self::Ghostty => self.build(cols, rows, profile),
            #[cfg(feature = "xtermjs")]
            Self::Xtermjs => self.build(cols, rows, profile),
        }
    }

    fn expected() -> String {
        Self::ALL
            .iter()
            .map(|backend| backend.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl std::str::FromStr for Backend {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "alacritty" => Ok(Self::Alacritty),
            #[cfg(feature = "ghostty")]
            "ghostty" => Ok(Self::Ghostty),
            #[cfg(feature = "xtermjs")]
            "xtermjs" => Ok(Self::Xtermjs),
            other => Err(format!(
                "unknown terminal backend {other:?}; expected one of: {}",
                Self::expected()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for &backend in Backend::ALL {
            assert_eq!(backend.as_str().parse(), Ok(backend));
        }
    }

    #[test]
    fn alacritty_remains_the_default() {
        assert_eq!(Backend::default(), Backend::Alacritty);
    }

    #[cfg(feature = "ghostty")]
    #[test]
    fn legacy_backend_spelling_is_rejected() {
        assert!("libghostty".parse::<Backend>().is_err());
        assert!(serde_json::from_str::<Backend>("\"libghostty\"").is_err());
    }

    #[test]
    fn every_enabled_backend_constructs() {
        for &backend in Backend::ALL {
            let mut emulator = backend
                .build(10, 2, &Profile::default())
                .unwrap_or_else(|error| panic!("{}: {error:#}", backend.as_str()));
            emulator.process(b"ok");
            assert_eq!(emulator.viewable_rows()[0][0].ch, "o");
        }
    }
}
