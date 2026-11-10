//! Application reports from the PTY stream, independent of the screen backend.

use crate::api::{ProgramStatus, ProgressBar, ProgressState};

const CAPABILITIES: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth";

#[derive(Default)]
enum Framing {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    Osc,
    OscEscape,
    String,
    StringEscape,
}

#[derive(Default)]
pub(crate) struct Reports {
    framing: Framing,
    osc: Vec<u8>,
    oversized: bool,
    state: ReportState,
    statuses: super::program_status::ProgramStatuses,
}

impl Reports {
    #[cfg(test)]
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            let _ = self.advance(byte);
        }
    }

    pub fn process(&mut self, emu: &mut dyn super::emu::Emulator, bytes: &[u8]) -> Vec<u8> {
        let mut pending = Vec::new();
        let mut start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            if let Some(st) = self.advance(byte) {
                // Drain earlier native replies before our reply, then let later
                // queries reach the backend. Native status callbacks stay unset.
                emu.process(&bytes[start..=index]);
                pending.extend(emu.take_pending_writes());
                pending.extend_from_slice(CAPABILITIES);
                pending.extend_from_slice(if st { b"\x1b\\" } else { b"\x07" });
                start = index + 1;
            }
        }
        if start < bytes.len() {
            emu.process(&bytes[start..]);
        }
        pending.extend(emu.take_pending_writes());
        pending
    }

    pub fn program_status(&self, id: &str) -> Option<ProgramStatus> {
        self.statuses.get(id)
    }

    pub fn program_statuses(&self) -> Vec<ProgramStatus> {
        self.statuses.all()
    }

    pub fn finish_process(&mut self) {
        self.statuses.finish_process();
    }

    fn advance(&mut self, byte: u8) -> Option<bool> {
        use Framing::*;
        if matches!(byte, 0x18 | 0x1a) {
            self.framing = Ground;
            self.osc.clear();
            return None;
        }
        let mut reply = None;
        match self.framing {
            Ground if byte == 0x1b => self.framing = Escape,
            Ground => {}
            Escape => self.escape(byte),
            EscapeIntermediate => match byte {
                0x1b => self.framing = Escape,
                0x30..=0x7e => self.framing = Ground,
                _ => {}
            },
            Osc => match byte {
                7 => reply = self.finish_osc(false),
                0x1b => self.framing = OscEscape,
                _ => {
                    if self.osc.len() < 4096 {
                        self.osc.push(byte);
                    } else {
                        self.oversized = true;
                    }
                }
            },
            OscEscape => {
                if byte == b'\\' {
                    reply = self.finish_osc(true);
                } else {
                    self.osc.clear();
                    self.escape(byte);
                }
            }
            String if byte == 0x1b => self.framing = StringEscape,
            String => {}
            StringEscape => self.escape(byte),
        }
        reply
    }

    fn escape(&mut self, byte: u8) {
        use Framing::*;
        self.framing = match byte {
            b']' => {
                self.osc.clear();
                self.oversized = false;
                Osc
            }
            b'P' | b'X' | b'^' | b'_' => String,
            b'c' => {
                self.state.progress = ProgressBar::default();
                self.statuses.clear();
                Ground
            }
            0x1b | 0..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => Escape,
            0x20..=0x2f => EscapeIntermediate,
            _ => Ground,
        };
    }

    fn finish_osc(&mut self, st: bool) -> Option<bool> {
        // Wait for the complete terminator. An ESC or cancellation alone
        // must not commit a partially received report.
        let mut reply = None;
        if !self.oversized {
            if let Some(body) = self.osc.strip_prefix(b"7501;") {
                if self.osc.len() + 2 + if st { 2 } else { 1 } <= 4096 {
                    if body == b"?" {
                        reply = Some(st);
                    } else {
                        self.statuses.report(body);
                    }
                }
            } else {
                let params: Vec<_> = self.osc.split(|byte| *byte == b';').collect();
                if matches!(params.as_slice(), [b"133" | b"633", b"A", ..]) {
                    self.statuses.finish_process();
                }
                self.state.osc_dispatch(&params);
            }
        }
        self.osc.clear();
        self.framing = Framing::Ground;
        reply
    }

    pub fn progress_bar(&self) -> ProgressBar {
        self.state.progress
    }
}

#[derive(Default)]
struct ReportState {
    progress: ProgressBar,
}

pub(super) fn percentage(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let value = std::str::from_utf8(bytes).ok()?.parse().ok()?;
    (value <= 100).then_some(value)
}

impl ReportState {
    fn osc_dispatch(&mut self, params: &[&[u8]]) {
        let [b"9", b"4", state, tail @ ..] = params else {
            return;
        };
        let value = match tail {
            [] => None,
            [value] => match percentage(value) {
                Some(value) => Some(value),
                None => return,
            },
            _ => return,
        };
        let state = match *state {
            b"0" => {
                self.progress = ProgressBar::default();
                return;
            }
            b"1" if value.is_some() => ProgressState::Normal,
            b"2" => ProgressState::Error,
            b"3" => ProgressState::Indeterminate,
            b"4" => ProgressState::Paused,
            _ => return,
        };
        self.progress.state = state;
        if state != ProgressState::Indeterminate {
            if let Some(value) = value {
                self.progress.progress = Some(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reports_survive_splits_and_follow_reset_and_prompt_rules() {
        for terminator in ["\x07", "\x1b\\"] {
            let report = format!("\x1b]7501;state=blocked:kind=auth:id=login{terminator}");
            for split in 0..=report.len() {
                let mut reports = Reports::default();
                reports.feed(&report.as_bytes()[..split]);
                reports.feed(&report.as_bytes()[split..]);
                assert_eq!(
                    reports.program_status("login").unwrap().state,
                    crate::api::ProgramState::Blocked
                );
                reports.feed(b"\x1b]7501;state=done:id=finished\x07");
                reports.feed(b"\x1b[?1049h\x1b[?1049l\x1b[!p");
                assert_eq!(reports.program_statuses().len(), 2);
                reports.feed(b"\x1b]133;A\x07");
                assert_eq!(reports.program_statuses().len(), 1);
                assert!(reports.program_status("finished").is_some());
                reports.feed(b"\x1bc");
                assert!(reports.program_statuses().is_empty());
            }
        }
    }

    #[test]
    fn status_limits_include_framing_and_canceled_reports_never_commit() {
        for terminator in ["\x07", "\x1b\\"] {
            let prefix = "\x1b]7501;state=done:unknown=";
            let exact = format!(
                "{prefix}{}{terminator}",
                "a".repeat(4096 - prefix.len() - terminator.len())
            );
            let mut reports = Reports::default();
            reports.feed(exact.as_bytes());
            assert_eq!(
                reports.program_status("").unwrap().state,
                crate::api::ProgramState::Done
            );
            let oversized = exact.replace("state=done", "state=error");
            reports.feed(oversized.as_bytes());
            assert_eq!(
                reports.program_status("").unwrap().state,
                crate::api::ProgramState::Done
            );
            for report in [
                b"\x1b]7501;state=working\x18".as_slice(),
                b"\x1b]7501;state=working\x1a",
                b"\x1b]7501;state=working\x1b[0m",
            ] {
                reports.feed(report);
                assert_eq!(
                    reports.program_status("").unwrap().state,
                    crate::api::ProgramState::Done
                );
            }
        }
    }

    #[test]
    fn capability_replies_are_unique_and_ordered_with_native_queries() {
        use crate::{profile::Profile, Backend};
        for &backend in Backend::ALL {
            for terminator in ["\x07", "\x1b\\"] {
                let mut emu = backend.build(80, 24, &Profile::default()).unwrap();
                emu.process(b"\x1b[c");
                let da = emu.take_pending_writes();
                assert!(!da.is_empty(), "{backend:?}");
                let input =
                    format!("\x1b[c\x1b]7501;?{terminator}\x1b[c\x1b]7501;?{terminator}TEXT");
                let reply = [CAPABILITIES, terminator.as_bytes()].concat();
                let expected = [
                    da.as_slice(),
                    reply.as_slice(),
                    da.as_slice(),
                    reply.as_slice(),
                ]
                .concat();
                for split in 0..=input.len() {
                    let mut reports = Reports::default();
                    let mut actual = reports.process(emu.as_mut(), &input.as_bytes()[..split]);
                    actual.extend(reports.process(emu.as_mut(), &input.as_bytes()[split..]));
                    assert_eq!(actual, expected, "{backend:?}, split {split}");
                }
            }
        }
    }

    #[test]
    fn reports_survive_every_chunk_boundary_and_both_terminators() {
        for bytes in [b"\x1b]9;4;1;42\x07".as_slice(), b"\x1b]9;4;1;42\x1b\\"] {
            for split in 0..=bytes.len() {
                let mut reports = Reports::default();
                reports.feed(&bytes[..split]);
                reports.feed(&bytes[split..]);
                assert_eq!(
                    reports.progress_bar(),
                    ProgressBar {
                        state: ProgressState::Normal,
                        progress: Some(42),
                    }
                );
            }
        }
    }

    #[test]
    fn transitions_preserve_or_replace_the_percentage() {
        let mut reports = Reports::default();
        for (sequence, state, progress) in [
            ("\x1b]9;4;1;30\x07", ProgressState::Normal, 30),
            ("\x1b]9;4;2\x07", ProgressState::Error, 30),
            ("\x1b]9;4;4;70\x07", ProgressState::Paused, 70),
            ("\x1b]9;4;3;10\x07", ProgressState::Indeterminate, 70),
            ("\x1b]9;4;4\x07", ProgressState::Paused, 70),
            ("\x1b]9;4;0\x07", ProgressState::Inactive, 0),
        ] {
            reports.feed(sequence.as_bytes());
            assert_eq!(
                reports.progress_bar(),
                ProgressBar {
                    state,
                    progress: Some(progress)
                }
            );
        }
    }

    #[test]
    fn malformed_reports_are_atomic_and_reset_is_explicit() {
        let mut reports = Reports::default();
        reports.feed(b"\x1b]9;4;1;50\x07");
        let expected = reports.progress_bar();
        for sequence in [
            "\x1b]9;4;5;60\x07",
            "\x1b]9;4;1\x07",
            "\x1b]9;4;2;101\x07",
            "\x1b]9;4;0;no\x07",
            "\x1b]9;4;4;-1\x07",
            "\x1b]9;4;2;5;6\x07",
            "\x1b]9;4;2;+5\x07",
            "\x1b]9;4;1;60\x18",
            "\x1b[?1049h\x1b[?1049l",
            "\x1b]9;4;1;60\x1b[0m",
            "\x1b]9;4;1;60\x1a",
            "\x1b[!p",
            "\x1bP9;4;1;70\x1b\\",
        ] {
            reports.feed(sequence.as_bytes());
            assert_eq!(reports.progress_bar(), expected, "{sequence:?}");
        }
        reports.feed(b"\x1bc");
        assert_eq!(reports.progress_bar(), ProgressBar::default());
    }

    #[test]
    fn st_requires_both_bytes_and_oversized_reports_do_not_update_state() {
        let mut reports = Reports::default();
        reports.feed(b"\x1b]9;4;1;60\x1b");
        assert_eq!(reports.progress_bar(), ProgressBar::default());
        reports.feed(b"\\");
        assert_eq!(reports.progress_bar().progress, Some(60));
        reports.feed(b"\x1b]9;4;1;");
        reports.feed(&vec![b'0'; 8192]);
        reports.feed(b"70\x07");
        assert_eq!(reports.progress_bar().progress, Some(60));
        reports.feed(b"\x1b]9;4;1;80\x07");
        assert_eq!(reports.progress_bar().progress, Some(80));
    }

    #[test]
    fn match_filters_validate_percentages_and_ignore_omitted_fields() {
        use crate::api::ProgressBarMatch;
        let actual = ProgressBar {
            state: ProgressState::Paused,
            progress: Some(30),
        };
        assert!(ProgressBarMatch {
            state: Some(ProgressState::Paused),
            progress: None
        }
        .matches(&actual));
        assert!(ProgressBarMatch {
            state: None,
            progress: Some(30)
        }
        .matches(&actual));
        assert!(!ProgressBarMatch {
            state: None,
            progress: Some(31)
        }
        .matches(&actual));
        assert!(ProgressBarMatch {
            state: None,
            progress: Some(101)
        }
        .validate()
        .is_err());
    }
}
