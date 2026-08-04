use serde::{Deserialize, Serialize};

use crate::shell::Shell;

/// Per-session default timeouts, one per [`crate::config::TimeoutClass`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeoutDefaults {
    pub text: Option<u64>,
    pub idle: Option<u64>,
    pub command: Option<u64>,
    pub exit: Option<u64>,
    pub ready: Option<u64>,
}

impl TimeoutDefaults {
    /// The default configured for `class`, if any.
    pub fn get(&self, class: crate::config::TimeoutClass) -> Option<u64> {
        use crate::config::TimeoutClass::*;
        match class {
            Text => self.text,
            Idle => self.idle,
            Command => self.command,
            Exit => self.exit,
            Ready => self.ready,
        }
    }
}

/// A terminal operation shared by native bindings and the cli adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Open {
        shell: Option<Shell>,
        program: Option<Vec<String>>,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        /// Whether to wait for readiness; `None` keeps the target default.
        #[serde(default)]
        wait_ready: Option<bool>,
        #[serde(default)]
        timeouts: TimeoutDefaults,
    },
    Close,
    Status,
    State,
    Text {
        full: bool,
    },
    Cells {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
    },
    Get {
        field: GetField,
    },
    Write {
        data: String,
    },
    Submit {
        data: Option<String>,
    },
    Press {
        keys: Vec<String>,
    },
    Mouse {
        action: MouseAction,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Signal {
        name: String,
    },
    WaitText {
        text: String,
        regex: bool,
        full: bool,
        #[serde(default)]
        timeout_ms: Option<u64>,
        not: bool,
    },
    WaitIdle {
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    WaitCommand {
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    WaitExit {
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    WaitReady {
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    ExpectText {
        text: String,
        regex: bool,
        full: bool,
        strict: bool,
        not: bool,
        fg: Option<String>,
        bg: Option<String>,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    ExpectExitCode {
        code: i32,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    ExpectOutput {
        text: String,
        regex: bool,
    },
    Snapshot {
        name: String,
        update: bool,
        include_colors: bool,
        /// The client's working directory; `__snapshots__` is resolved against
        /// it so snapshots land next to the caller, not the daemon.
        #[serde(default)]
        cwd: Option<String>,
    },
    Screenshot {
        full: bool,
        path: Option<String>,
    },
    Monitor {
        cols: u16,
        rows: u16,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GetField {
    Command,
    Output,
    ExitCode,
    Cwd,
    Cursor,
    Size,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MouseAction {
    Click {
        x: Option<u16>,
        y: Option<u16>,
        on_text: Option<String>,
        button: u8,
        clicks: u8,
    },
    Move {
        x: u16,
        y: u16,
    },
    Down {
        x: u16,
        y: u16,
        button: u8,
    },
    Up {
        x: u16,
        y: u16,
        button: u8,
    },
    Drag {
        x1: u16,
        y1: u16,
        x2: u16,
        y2: u16,
        button: u8,
    },
    Scroll {
        direction: String,
        amount: u16,
    },
}

/// Classifies a failure so the cli can map it to a stable process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// An assertion or wait condition was not met (e.g. `expect`/`wait`).
    Assertion,
    /// An invalid argument value reached the daemon (e.g. bad regex/color).
    Usage,
    /// No active session for the target (run `open`/`run` first).
    NoSession,
    /// An internal error (spawn, I/O, rendering, ...).
    Internal,
}

impl ErrorKind {
    /// Stable process exit code for this failure class.
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Assertion => 1,
            ErrorKind::Usage => 2,
            ErrorKind::NoSession => 3,
            ErrorKind::Internal => 5,
        }
    }
}

/// A terminal operation result shared by native bindings and the cli adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    /// Human/JSON payload describing the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Error or assertion-failure message when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Failure classification when `ok` is false; drives the cli exit code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ErrorKind>,
}

impl Response {
    pub fn ok() -> Self {
        Response {
            ok: true,
            data: None,
            message: None,
            kind: None,
        }
    }

    pub fn with(data: serde_json::Value) -> Self {
        Response {
            ok: true,
            data: Some(data),
            message: None,
            kind: None,
        }
    }

    /// A failure of the given class.
    pub fn err(kind: ErrorKind, message: impl Into<String>) -> Self {
        Response {
            ok: false,
            data: None,
            message: Some(message.into()),
            kind: Some(kind),
        }
    }

    /// An assertion / wait failure (exit code 1).
    pub fn assertion(message: impl Into<String>) -> Self {
        Response::err(ErrorKind::Assertion, message)
    }

    /// An invalid-argument failure (exit code 2).
    pub fn usage(message: impl Into<String>) -> Self {
        Response::err(ErrorKind::Usage, message)
    }

    /// A "no active session" failure (exit code 3).
    pub fn no_session() -> Self {
        Response::err(
            ErrorKind::NoSession,
            "no active session; run `shell-use open` (or `shell-use run <program>`) first",
        )
    }

    /// An internal failure (exit code 5).
    pub fn internal(message: impl Into<String>) -> Self {
        Response::err(ErrorKind::Internal, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_open_req(wait_ready: Option<bool>, timeouts: TimeoutDefaults) -> Request {
        Request::Open {
            shell: None,
            program: None,
            cols: 80,
            rows: 30,
            cwd: None,
            env: vec![],
            wait_ready,
            timeouts,
        }
    }

    /// Clients released before `wait_ready` existed must still deserialize.
    #[test]
    fn open_without_wait_ready_still_deserializes() {
        let raw = r#"{"kind":"open","shell":null,"program":null,"cols":80,"rows":30,
                      "cwd":null,"env":[]}"#;
        let req: Request = serde_json::from_str(raw).expect("deserialize legacy open");
        match req {
            Request::Open {
                wait_ready,
                cols,
                timeouts,
                ..
            } => {
                assert_eq!(wait_ready, None);
                assert_eq!(cols, 80);
                assert_eq!(
                    timeouts,
                    TimeoutDefaults::default(),
                    "an absent timeouts object means nothing is configured"
                );
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }

    /// Older clients' concrete `timeout_ms` must remain an explicit override.
    #[test]
    fn waits_accept_a_concrete_timeout_from_older_clients() {
        let raw = r#"{"kind":"wait_idle","timeout_ms":1234}"#;
        match serde_json::from_str::<Request>(raw).expect("deserialize wait_idle") {
            Request::WaitIdle { timeout_ms } => assert_eq!(timeout_ms, Some(1234)),
            other => panic!("expected WaitIdle, got {other:?}"),
        }
    }

    #[test]
    fn waits_treat_an_absent_timeout_as_unset() {
        for raw in [
            r#"{"kind":"wait_idle"}"#,
            r#"{"kind":"wait_command"}"#,
            r#"{"kind":"wait_exit"}"#,
            r#"{"kind":"wait_ready"}"#,
        ] {
            let req: Request = serde_json::from_str(raw).expect("deserialize wait");
            let timeout = match req {
                Request::WaitIdle { timeout_ms }
                | Request::WaitCommand { timeout_ms }
                | Request::WaitExit { timeout_ms }
                | Request::WaitReady { timeout_ms } => timeout_ms,
                other => panic!("expected a wait, got {other:?}"),
            };
            assert_eq!(timeout, None, "{raw} should leave the timeout unset");
        }
    }

    /// `expect exit-code` gained a timeout; older payloads omit it.
    #[test]
    fn expect_exit_code_timeout_is_optional() {
        let raw = r#"{"kind":"expect_exit_code","code":0}"#;
        match serde_json::from_str::<Request>(raw).expect("deserialize expect_exit_code") {
            Request::ExpectExitCode { code, timeout_ms } => {
                assert_eq!(code, 0);
                assert_eq!(timeout_ms, None);
            }
            other => panic!("expected ExpectExitCode, got {other:?}"),
        }
    }

    #[test]
    fn open_round_trips_session_timeout_defaults() {
        let timeouts = TimeoutDefaults {
            text: Some(30_000),
            idle: Some(15_000),
            ready: Some(20_000),
            ..TimeoutDefaults::default()
        };
        let req = make_open_req(None, timeouts);
        let encoded = serde_json::to_string(&req).expect("serialize open");
        match serde_json::from_str::<Request>(&encoded).expect("deserialize open") {
            Request::Open { timeouts: got, .. } => {
                assert_eq!(got, timeouts);
                assert_eq!(got.get(crate::config::TimeoutClass::Text), Some(30_000));
                assert_eq!(got.get(crate::config::TimeoutClass::Command), None);
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }

    #[test]
    fn open_round_trips_an_explicit_wait_ready() {
        for expected in [Some(true), Some(false), None] {
            let req = make_open_req(expected, TimeoutDefaults::default());
            let encoded = serde_json::to_string(&req).expect("serialize open");
            let decoded: Request = serde_json::from_str(&encoded).expect("deserialize open");
            match decoded {
                Request::Open { wait_ready, .. } => assert_eq!(wait_ready, expected),
                other => panic!("expected Open, got {other:?}"),
            }
        }
    }

    #[test]
    fn wait_ready_uses_a_snake_case_kind() {
        let req: Request = serde_json::from_str(r#"{"kind":"wait_ready","timeout_ms":1234}"#)
            .expect("deserialize wait_ready");
        match req {
            Request::WaitReady { timeout_ms } => assert_eq!(timeout_ms, Some(1234)),
            other => panic!("expected WaitReady, got {other:?}"),
        }
    }
}
