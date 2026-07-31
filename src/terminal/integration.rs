//! Command and output tracking from the PTY byte stream.

use rio_vt::performer::parser::{Parser, Perform};

const OSC_CMD: &[u8] = b"133";
const OSC_VSCODE: &[u8] = b"633";
const OSC_CWD: &[u8] = b"7";

#[derive(Default, PartialEq, Clone, Copy)]
enum Region {
    #[default]
    None,
    Command,
    Output,
}

#[derive(Default)]
struct TrackerState {
    region: Region,
    command_buf: String,
    output_buf: String,

    started: bool,
    prompt_active: bool,
    last_exit: Option<i32>,
    cwd: Option<String>,
    last_command: Option<String>,
    last_output: Option<String>,
    finished_count: u64,
}

/// Tracks command boundaries, exit codes, and cwd from the PTY byte stream.
pub struct CommandTracker {
    parser: Parser,
    state: TrackerState,
}

impl Default for CommandTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandTracker {
    pub fn new() -> Self {
        CommandTracker {
            parser: Parser::new(),
            state: TrackerState::default(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
    }

    /// Whether the shell is sitting at an idle, ready prompt.
    pub fn is_ready(&self) -> bool {
        self.state.prompt_active
    }

    /// Whether at least one prompt has been seen.
    pub fn started(&self) -> bool {
        self.state.started
    }

    pub fn finished_count(&self) -> u64 {
        self.state.finished_count
    }

    pub fn last_exit(&self) -> Option<i32> {
        self.state.last_exit
    }

    pub fn cwd(&self) -> Option<&str> {
        self.state.cwd.as_deref()
    }

    pub fn last_command(&self) -> Option<&str> {
        self.state.last_command.as_deref()
    }

    pub fn last_output(&self) -> Option<&str> {
        self.state.last_output.as_deref()
    }
}

impl TrackerState {
    fn command_marker(&mut self, marker: &str, exit: Option<&str>) {
        match marker {
            "A" => {
                self.started = true;
                self.region = Region::None;
            }
            "B" => {
                self.started = true;
                self.prompt_active = true;
                self.region = Region::Command;
                self.command_buf.clear();
            }
            "C" => {
                self.prompt_active = false;
                let cmd = clean(&self.command_buf);
                let cmd = cmd.strip_prefix("> ").unwrap_or(&cmd).to_string();
                self.last_command = Some(cmd);
                self.region = Region::Output;
                self.output_buf.clear();
            }
            "D" => {
                self.last_output = Some(clean(&self.output_buf));
                self.region = Region::None;
                self.last_exit = exit.and_then(|s| s.trim().parse::<i32>().ok());
                self.finished_count += 1;
            }
            _ => {}
        }
    }
}

impl Perform for TrackerState {
    fn print(&mut self, c: char) {
        match self.region {
            Region::Command => self.command_buf.push(c),
            Region::Output => self.output_buf.push(c),
            Region::None => {}
        }
    }

    fn execute(&mut self, byte: u8) {
        if byte == b'\n' && self.region == Region::Output {
            self.output_buf.push('\n');
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        match params.first().copied() {
            Some(OSC_CMD) | Some(OSC_VSCODE) => {
                let Some(marker) = params.get(1).and_then(|m| std::str::from_utf8(m).ok()) else {
                    return;
                };
                let exit = params.get(2).and_then(|p| std::str::from_utf8(p).ok());
                self.command_marker(marker, exit);
            }
            Some(OSC_CWD) if params.len() > 1 => {
                let value = params[1..]
                    .iter()
                    .map(|p| String::from_utf8_lossy(p))
                    .collect::<Vec<_>>()
                    .join(";");
                if let Some(path) = parse_file_url(&value) {
                    self.cwd = Some(path);
                }
            }
            _ => {}
        }
    }
}

fn clean(s: &str) -> String {
    s.trim_matches([' ', '\n', '\r']).to_string()
}

fn parse_file_url(value: &str) -> Option<String> {
    let rest = value.strip_prefix("file://")?;
    let slash = rest.find('/')?;
    let path = percent_decode(&rest[slash..]);
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        Some(path[1..].to_string())
    } else {
        Some(path)
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn osc(marker: &str) -> Vec<u8> {
        format!("\x1b]133;{marker}\x07").into_bytes()
    }

    fn osc633(marker: &str) -> Vec<u8> {
        format!("\x1b]633;{marker}\x07").into_bytes()
    }

    #[test]
    fn tracks_full_command_cycle() {
        let mut t = CommandTracker::new();
        t.feed(&osc("A"));
        t.feed(b"> ");
        t.feed(&osc("B"));
        assert!(t.is_ready());
        assert!(t.started());
        t.feed(b"echo hi");
        t.feed(&osc("C"));
        assert!(!t.is_ready());
        assert_eq!(t.last_command(), Some("echo hi"));
        t.feed(b"hi\r\n");
        t.feed(&osc("D;0"));
        assert_eq!(t.last_exit(), Some(0));
        assert_eq!(t.finished_count(), 1);
        assert_eq!(t.last_output(), Some("hi"));
    }

    #[test]
    fn parses_exit_code() {
        let mut t = CommandTracker::new();
        t.feed(&osc("B"));
        t.feed(&osc("C"));
        t.feed(&osc("D;7"));
        assert_eq!(t.last_exit(), Some(7));
    }

    #[test]
    fn finished_without_exit_code() {
        let mut t = CommandTracker::new();
        t.feed(&osc("B"));
        t.feed(&osc("C"));
        t.feed(&osc("D"));
        assert_eq!(t.last_exit(), None);
        assert_eq!(t.finished_count(), 1);
    }

    #[test]
    fn cwd_via_osc7_file_url() {
        let mut t = CommandTracker::new();
        t.feed(b"\x1b]7;file://myhost/home/x\x07");
        assert_eq!(t.cwd(), Some("/home/x"));
    }

    #[test]
    fn cwd_osc7_windows_drive_and_percent() {
        let mut t = CommandTracker::new();
        t.feed(b"\x1b]7;file:///C:/Users/My%20Code\x1b\\");
        assert_eq!(t.cwd(), Some("C:/Users/My Code"));
    }

    #[test]
    fn ignores_unrelated_osc() {
        let mut t = CommandTracker::new();
        t.feed(b"\x1b]0;window title\x07");
        assert!(!t.started());
    }

    #[test]
    fn prompt_prefix_not_captured_in_command() {
        let mut t = CommandTracker::new();
        t.feed(b"> ");
        t.feed(&osc("B"));
        t.feed(b"echo hello");
        t.feed(&osc("C"));
        assert_eq!(t.last_command(), Some("echo hello"));
    }

    #[test]
    fn prompt_prefix_stripped_when_repainted_into_command() {
        let mut t = CommandTracker::new();
        t.feed(&osc("B"));
        t.feed(b"> echo hello");
        t.feed(&osc("C"));
        assert_eq!(t.last_command(), Some("echo hello"));
    }

    #[test]
    fn osc_terminated_by_st_not_just_bel() {
        let mut t = CommandTracker::new();
        t.feed(b"\x1b]133;D;3\x1b\\");
        assert_eq!(t.last_exit(), Some(3));
    }

    #[test]
    fn marker_split_across_feeds() {
        let mut t = CommandTracker::new();
        t.feed(b"\x1b]133;");
        t.feed(b"D;5\x07");
        assert_eq!(t.last_exit(), Some(5));
    }

    #[test]
    fn output_strips_embedded_csi_colors() {
        let mut t = CommandTracker::new();
        t.feed(&osc("B"));
        t.feed(&osc("C"));
        t.feed(b"a\x1b[31mb\x1b[0mc");
        t.feed(&osc("D;0"));
        assert_eq!(t.last_output(), Some("abc"));
    }

    #[test]
    fn output_ignores_dcs_payload() {
        let mut t = CommandTracker::new();
        t.feed(&osc("B"));
        t.feed(&osc("C"));
        t.feed(b"x\x1bP1;2;3qSIXELDATA\x1b\\y");
        t.feed(&osc("D;0"));
        assert_eq!(t.last_output(), Some("xy"));
    }

    #[test]
    fn osc_like_bytes_inside_dcs_do_not_trigger() {
        let mut t = CommandTracker::new();
        t.feed(b"\x1bP133;D;99\x1b\\");
        assert_eq!(t.last_exit(), None);
        assert_eq!(t.finished_count(), 0);
    }

    #[test]
    fn osc633_full_cycle_with_exit() {
        let mut t = CommandTracker::new();
        t.feed(&osc633("A"));
        t.feed(&osc633("B"));
        assert!(t.is_ready());
        t.feed(&osc633("C"));
        assert!(!t.is_ready());
        t.feed(b"out\r\n");
        t.feed(&osc633("D;5"));
        assert_eq!(t.last_exit(), Some(5));
        assert_eq!(t.finished_count(), 1);
        assert_eq!(t.last_output(), Some("out"));
    }

    #[test]
    fn osc633_e_and_p_are_ignored() {
        let mut t = CommandTracker::new();
        t.feed(&osc633("B"));
        t.feed(b"scraped cmd");
        t.feed(&osc633("C"));
        t.feed(&osc633("D;0"));
        t.feed(&osc633("E;totally different"));
        t.feed(b"\x1b]633;P;Cwd=C:/should/be/ignored\x07");
        assert_eq!(t.last_command(), Some("scraped cmd"));
        assert_eq!(t.cwd(), None);
    }

    #[test]
    fn osc633_a_with_extra_param_ignored() {
        let mut t = CommandTracker::new();
        t.feed(b"\x1b]633;A;k=i\x07");
        assert!(t.started());
    }
}
