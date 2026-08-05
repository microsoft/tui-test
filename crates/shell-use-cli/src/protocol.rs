use serde::{Deserialize, Serialize};
use serde_json::json;

use shell_use::{
    Engine, OpenOptions, Operation, OperationResult, RunOptions, ScreenshotResult, ShellUseError,
};

pub use shell_use::{ErrorKind, MouseAction, Timeouts};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Open {
        shell: Option<shell_use::shell::Shell>,
        program: Option<Vec<String>>,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        #[serde(default)]
        wait_ready: Option<bool>,
        #[serde(default)]
        timeouts: Timeouts,
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
    WaitBell {
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
    ExpectBellCount {
        count: u64,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    Snapshot {
        name: String,
        update: bool,
        include_colors: bool,
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

impl Request {
    pub fn execute(self, engine: &Engine) -> Response {
        match self.into_operation() {
            Ok(operation) => Response::from_result(engine.execute(operation)),
            Err(error) => Response::from_error(error),
        }
    }

    fn into_operation(self) -> Result<Operation, ShellUseError> {
        match self {
            Request::Open {
                shell,
                program,
                cols,
                rows,
                cwd,
                env,
                wait_ready,
                timeouts,
            } => {
                if let Some(program) = program {
                    let mut parts = program.into_iter();
                    let executable = parts
                        .next()
                        .ok_or_else(|| ShellUseError::usage("empty program"))?;
                    Ok(Operation::Run(RunOptions {
                        program: executable,
                        args: parts.collect(),
                        cols,
                        rows,
                        cwd,
                        env,
                        wait_ready,
                        timeouts,
                    }))
                } else {
                    Ok(Operation::Open(OpenOptions {
                        shell,
                        cols,
                        rows,
                        cwd,
                        env,
                        wait_ready,
                        timeouts,
                    }))
                }
            }
            Request::Close => Ok(Operation::Close),
            Request::State => Ok(Operation::State),
            Request::Text { full } => Ok(Operation::Text { full }),
            Request::Cells { x, y, w, h } => Ok(Operation::Cells { x, y, w, h }),
            Request::Get { field } => Ok(match field {
                GetField::Command => Operation::GetCommand,
                GetField::Output => Operation::GetOutput,
                GetField::ExitCode => Operation::GetExitCode,
                GetField::Cwd => Operation::GetCwd,
                GetField::Cursor => Operation::GetCursor,
                GetField::Size => Operation::GetSize,
                GetField::BellCount => Operation::GetBellCount,
            }),
            Request::Write { data } => Ok(Operation::Write { data }),
            Request::Submit { data } => Ok(Operation::Submit { data }),
            Request::Press { keys } => Ok(Operation::Press { keys }),
            Request::Mouse { action } => Ok(Operation::Mouse { action }),
            Request::Resize { cols, rows } => Ok(Operation::Resize { cols, rows }),
            Request::Signal { name } => Ok(Operation::Signal { name }),
            Request::WaitText {
                text,
                regex,
                full,
                timeout_ms,
                not,
            } => Ok(Operation::WaitText {
                text,
                regex,
                full,
                timeout_ms,
                not,
            }),
            Request::WaitIdle { timeout_ms } => Ok(Operation::WaitIdle { timeout_ms }),
            Request::WaitCommand { timeout_ms } => Ok(Operation::WaitCommand { timeout_ms }),
            Request::WaitExit { timeout_ms } => Ok(Operation::WaitExit { timeout_ms }),
            Request::WaitReady { timeout_ms } => Ok(Operation::WaitReady { timeout_ms }),
            Request::WaitBell { timeout_ms } => Ok(Operation::WaitBell { timeout_ms }),
            Request::ExpectText {
                text,
                regex,
                full,
                strict,
                not,
                fg,
                bg,
                timeout_ms,
            } => Ok(Operation::ExpectText {
                text,
                regex,
                full,
                strict,
                not,
                fg,
                bg,
                timeout_ms,
            }),
            Request::ExpectExitCode { code, timeout_ms } => {
                Ok(Operation::ExpectExitCode { code, timeout_ms })
            }
            Request::ExpectOutput { text, regex } => Ok(Operation::ExpectOutput { text, regex }),
            Request::ExpectBellCount { count, timeout_ms } => {
                Ok(Operation::ExpectBellCount { count, timeout_ms })
            }
            Request::Snapshot {
                name,
                update,
                include_colors,
                cwd,
            } => Ok(Operation::Snapshot {
                name,
                update,
                include_colors,
                cwd,
            }),
            Request::Screenshot { full, path } => Ok(Operation::Screenshot { full, path }),
            Request::Ping | Request::Status | Request::Monitor { .. } | Request::Shutdown => {
                Err(ShellUseError::usage(
                    "daemon control request cannot execute as a terminal operation",
                ))
            }
        }
    }
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
    BellCount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ErrorKind>,
}

impl Response {
    pub fn ok() -> Self {
        Self {
            ok: true,
            data: None,
            message: None,
            kind: None,
        }
    }

    pub fn with(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            message: None,
            kind: None,
        }
    }

    pub fn from_result(result: Result<OperationResult, ShellUseError>) -> Self {
        match result {
            Ok(result) => match operation_data(result) {
                Ok(Some(data)) => Self::with(data),
                Ok(None) => Self::ok(),
                Err(error) => Self::from_error(error),
            },
            Err(error) => Self::from_error(error),
        }
    }

    pub fn from_error(error: ShellUseError) -> Self {
        Self {
            ok: false,
            data: None,
            message: Some(error.message),
            kind: Some(error.kind),
        }
    }
}

fn operation_data(result: OperationResult) -> Result<Option<serde_json::Value>, ShellUseError> {
    let value = match result {
        OperationResult::Unit => return Ok(None),
        OperationResult::Open(value) => serde_json::to_value(value),
        OperationResult::State(value) => serde_json::to_value(value),
        OperationResult::Text(text) => Ok(json!({ "text": text })),
        OperationResult::PackedScreen(screen) => Ok(json!({
            "cols": screen.cols,
            "rows": screen.rows,
            "text": String::from_utf8_lossy(&screen.utf8),
        })),
        OperationResult::Cells(cells) => Ok(json!({ "cells": cells })),
        OperationResult::Command(value) => Ok(json!({ "value": value })),
        OperationResult::Output(value) => Ok(json!({ "value": value })),
        OperationResult::ExitCode(value) => Ok(json!({ "value": value })),
        OperationResult::Cwd(value) => Ok(json!({ "value": value })),
        OperationResult::Cursor(value) => Ok(json!({ "value": value })),
        OperationResult::Size(value) => Ok(json!({ "value": value })),
        OperationResult::BellCount(value) => Ok(json!({ "value": value })),
        OperationResult::Snapshot(status) => Ok(json!({ "status": status })),
        OperationResult::Screenshot(ScreenshotResult::Path(path)) => Ok(json!({ "path": path })),
        OperationResult::Screenshot(ScreenshotResult::Text(text)) => Ok(json!({ "text": text })),
    }
    .map_err(|error| ShellUseError::internal(format!("failed to encode cli response: {error}")))?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_open_req(wait_ready: Option<bool>, timeouts: Timeouts) -> Request {
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

    #[test]
    fn open_without_wait_ready_still_deserializes() {
        let raw = r#"{"kind":"open","shell":null,"program":null,"cols":80,"rows":30,
                      "cwd":null,"env":[]}"#;
        let request: Request = serde_json::from_str(raw).expect("deserialize legacy open");
        match request {
            Request::Open {
                wait_ready,
                cols,
                timeouts,
                ..
            } => {
                assert_eq!(wait_ready, None);
                assert_eq!(cols, 80);
                assert_eq!(timeouts, Timeouts::default());
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }

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
            r#"{"kind":"wait_bell"}"#,
        ] {
            let request: Request = serde_json::from_str(raw).expect("deserialize wait");
            let timeout = match request {
                Request::WaitIdle { timeout_ms }
                | Request::WaitCommand { timeout_ms }
                | Request::WaitExit { timeout_ms }
                | Request::WaitReady { timeout_ms }
                | Request::WaitBell { timeout_ms } => timeout_ms,
                other => panic!("expected a wait, got {other:?}"),
            };
            assert_eq!(timeout, None);
        }
    }

    #[test]
    fn bell_expectation_timeout_is_optional() {
        let raw = r#"{"kind":"expect_bell_count","count":2}"#;
        match serde_json::from_str::<Request>(raw).expect("deserialize expect_bell_count") {
            Request::ExpectBellCount { count, timeout_ms } => {
                assert_eq!(count, 2);
                assert_eq!(timeout_ms, None);
            }
            other => panic!("expected ExpectBellCount, got {other:?}"),
        }
    }

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
        let timeouts = Timeouts {
            text: Some(30_000),
            idle: Some(15_000),
            ready: Some(20_000),
            ..Timeouts::default()
        };
        let request = make_open_req(None, timeouts);
        let encoded = serde_json::to_string(&request).expect("serialize open");
        match serde_json::from_str::<Request>(&encoded).expect("deserialize open") {
            Request::Open { timeouts: got, .. } => {
                assert_eq!(got, timeouts);
                assert_eq!(got.get(shell_use::config::TimeoutClass::Text), Some(30_000));
                assert_eq!(got.get(shell_use::config::TimeoutClass::Command), None);
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }

    #[test]
    fn open_round_trips_an_explicit_wait_ready() {
        for expected in [Some(true), Some(false), None] {
            let request = make_open_req(expected, Timeouts::default());
            let encoded = serde_json::to_string(&request).expect("serialize open");
            let decoded: Request = serde_json::from_str(&encoded).expect("deserialize open");
            match decoded {
                Request::Open { wait_ready, .. } => assert_eq!(wait_ready, expected),
                other => panic!("expected Open, got {other:?}"),
            }
        }
    }
}
