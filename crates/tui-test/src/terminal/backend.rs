//! Terminal emulator selection and construction.

use serde::{Deserialize, Serialize};

use crate::event::BellTracker;
use crate::profile::Profile;
use crate::terminal::alacritty::AlacrittyEmu;
use crate::terminal::emu::Emulator;

#[cfg(feature = "rio")]
use crate::terminal::rio::RioEmu;

#[cfg(feature = "ghostty")]
use crate::terminal::ghostty::GhosttyEmu;
#[cfg(all(windows, feature = "windows-terminal"))]
use crate::terminal::windows_terminal::WindowsTerminalEmu;
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
    #[cfg(feature = "rio")]
    Rio,
    #[cfg(feature = "xtermjs")]
    Xtermjs,
    #[cfg(all(windows, feature = "windows-terminal"))]
    #[serde(rename = "windows-terminal")]
    WindowsTerminal,
}

impl Backend {
    pub const ALL: &'static [Self] = &[
        Self::Alacritty,
        #[cfg(feature = "ghostty")]
        Self::Ghostty,
        #[cfg(feature = "rio")]
        Self::Rio,
        #[cfg(feature = "xtermjs")]
        Self::Xtermjs,
        #[cfg(all(windows, feature = "windows-terminal"))]
        Self::WindowsTerminal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alacritty => "alacritty",
            #[cfg(feature = "ghostty")]
            Self::Ghostty => "ghostty",
            #[cfg(feature = "rio")]
            Self::Rio => "rio",
            #[cfg(feature = "xtermjs")]
            Self::Xtermjs => "xtermjs",
            #[cfg(all(windows, feature = "windows-terminal"))]
            Self::WindowsTerminal => "windows-terminal",
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
            #[cfg(feature = "rio")]
            Self::Rio => Ok(Box::new(RioEmu::new(cols, rows, profile))),
            #[cfg(feature = "xtermjs")]
            Self::Xtermjs => Ok(Box::new(XtermJsEmu::new(cols, rows, profile)?)),
            #[cfg(all(windows, feature = "windows-terminal"))]
            Self::WindowsTerminal => Ok(Box::new(WindowsTerminalEmu::new(cols, rows, profile)?)),
        }
    }

    /// Like [`Self::build`], but wires up bell tracking.
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
            #[cfg(feature = "ghostty")]
            Self::Ghostty => Ok(Box::new(GhosttyEmu::with_bell_tracker(
                cols, rows, profile, bells,
            )?)),
            #[cfg(feature = "rio")]
            Self::Rio => Ok(Box::new(RioEmu::with_bell_tracker(
                cols, rows, profile, bells,
            ))),
            #[cfg(feature = "xtermjs")]
            Self::Xtermjs => Ok(Box::new(XtermJsEmu::with_bell_tracker(
                cols, rows, profile, bells,
            )?)),
            #[cfg(all(windows, feature = "windows-terminal"))]
            Self::WindowsTerminal => Ok(Box::new(WindowsTerminalEmu::with_bell_tracker(
                cols, rows, profile, bells,
            )?)),
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
            #[cfg(feature = "rio")]
            "rio" => Ok(Self::Rio),
            #[cfg(feature = "xtermjs")]
            "xtermjs" => Ok(Self::Xtermjs),
            #[cfg(all(windows, feature = "windows-terminal"))]
            "windows-terminal" => Ok(Self::WindowsTerminal),
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

    #[cfg(not(all(windows, feature = "windows-terminal")))]
    #[test]
    fn windows_terminal_is_rejected_when_unavailable() {
        assert!("windows-terminal".parse::<Backend>().is_err());
        assert!(serde_json::from_str::<Backend>("\"windows-terminal\"").is_err());
    }

    #[cfg(all(windows, feature = "windows-terminal"))]
    #[test]
    fn windows_terminal_uses_the_hyphenated_name() {
        assert_eq!("windows-terminal".parse(), Ok(Backend::WindowsTerminal));
        assert_eq!(
            serde_json::to_string(&Backend::WindowsTerminal).unwrap(),
            "\"windows-terminal\""
        );
        assert_eq!(
            serde_json::from_str::<Backend>("\"windows-terminal\"").unwrap(),
            Backend::WindowsTerminal
        );
        assert!("windowsterminal".parse::<Backend>().is_err());
    }

    #[cfg(not(feature = "rio"))]
    #[test]
    fn rio_is_rejected_when_its_feature_is_disabled() {
        assert!("rio".parse::<Backend>().is_err());
        assert!(serde_json::from_str::<Backend>("\"rio\"").is_err());
    }

    #[cfg(not(feature = "xtermjs"))]
    #[test]
    fn xtermjs_is_rejected_when_its_feature_is_disabled() {
        assert!("xtermjs".parse::<Backend>().is_err());
        assert!(serde_json::from_str::<Backend>("\"xtermjs\"").is_err());
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
