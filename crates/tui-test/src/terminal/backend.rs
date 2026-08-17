//! Terminal emulator selection and construction.

use serde::{Deserialize, Serialize};

use crate::profile::Profile;
use crate::terminal::alacritty::AlacrittyEmu;
use crate::terminal::emu::Emulator;

#[cfg(feature = "ghostty")]
use crate::terminal::ghostty::GhosttyEmu;

/// The terminal emulator a session uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    Alacritty,
    #[cfg(feature = "ghostty")]
    Ghostty,
}

impl Backend {
    #[cfg(not(feature = "ghostty"))]
    pub const ALL: [Self; 1] = [Self::Alacritty];
    #[cfg(feature = "ghostty")]
    pub const ALL: [Self; 2] = [Self::Alacritty, Self::Ghostty];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alacritty => "alacritty",
            #[cfg(feature = "ghostty")]
            Self::Ghostty => "ghostty",
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
        }
    }

    const fn expected() -> &'static str {
        if cfg!(feature = "ghostty") {
            "alacritty, ghostty"
        } else {
            "alacritty"
        }
    }
}

impl std::str::FromStr for Backend {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "alacritty" => Ok(Self::Alacritty),
            #[cfg(feature = "ghostty")]
            "ghostty" => Ok(Self::Ghostty),
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
        for backend in Backend::ALL {
            assert_eq!(backend.as_str().parse(), Ok(backend));
        }
    }

    #[test]
    fn alacritty_remains_the_default() {
        assert_eq!(Backend::default(), Backend::Alacritty);
    }

    #[test]
    fn every_enabled_backend_constructs() {
        for backend in Backend::ALL {
            let mut emulator = backend
                .build(10, 2, &Profile::default())
                .unwrap_or_else(|error| panic!("{}: {error:#}", backend.as_str()));
            emulator.process(b"ok");
            assert_eq!(emulator.viewable_rows()[0][0].ch, "o");
        }
    }
}
