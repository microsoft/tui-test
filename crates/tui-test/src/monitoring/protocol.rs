//! The monitor-only wire contract; ordinary terminal commands remain in the CLI.

use crate::{ErrorKind, TuiTestError};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub session: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attach {
    #[serde(default)]
    pub protocol: u32,
    pub cols: u16,
    pub rows: u16,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<Route>,
}

impl Attach {
    pub fn new(cols: u16, rows: u16, interactive: bool) -> Self {
        Self {
            protocol: VERSION,
            cols,
            rows,
            interactive,
            route: None,
        }
    }

    pub fn validate(&self) -> Result<(), TuiTestError> {
        if self.protocol != VERSION {
            return Err(TuiTestError::usage(
                "incompatible monitor protocol; restart or upgrade the session owner",
            ));
        }
        if self.cols == 0 || self.rows == 0 {
            return Err(TuiTestError::usage(
                "monitor dimensions must be greater than zero",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Ping,
    HostSessions,
    Monitor(Attach),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MonitorReady {
    pub protocol: u32,
    pub frame: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonitorInput {
    Write { data: Vec<u8> },
    Resize { cols: u16, rows: u16 },
    Detach,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonitorOutput {
    Frame {
        frame: String,
    },
    Error {
        message: String,
        error_kind: ErrorKind,
    },
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSession {
    pub id: String,
    pub session: String,
    pub generation: u64,
    pub owner: String,
    pub pid: u32,
    pub label: Option<String>,
    pub test_file: Option<String>,
    pub test_name: Option<String>,
    pub framework: Option<String>,
    pub worker: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub status: String,
    pub outcome: Option<String>,
    pub child_exited: bool,
    pub exit_code: Option<i32>,
    pub clients: u32,
    #[serde(default)]
    pub interactive_clients: u32,
    pub started_at: u64,
    #[serde(default)]
    pub completed_at: Option<u64>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    pub protocol: u32,
    pub owner: String,
    pub capabilities: Vec<String>,
    pub sessions: Vec<HostSession>,
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
            data: Some(data),
            ..Self::ok()
        }
    }

    pub fn from_error(error: TuiTestError) -> Self {
        Self {
            ok: false,
            data: None,
            message: Some(error.message),
            kind: Some(error.kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_attach_is_one_versioned_routed_handshake() {
        let mut attach = Attach::new(80, 24, true);
        attach.route = Some(Route {
            session: "login".into(),
            generation: 7,
        });
        let json = serde_json::to_value(Request::Monitor(attach)).unwrap();
        assert_eq!(json["kind"], "monitor");
        assert_eq!(json["route"]["generation"], 7);
        let Request::Monitor(attach) = serde_json::from_value(json).unwrap() else {
            panic!("monitor request")
        };
        attach.validate().unwrap();
    }

    #[test]
    fn invalid_attach_versions_and_dimensions_are_rejected() {
        let legacy: Attach = serde_json::from_str(r#"{"cols":80,"rows":24}"#).unwrap();
        assert_eq!(legacy.validate().unwrap_err().kind, ErrorKind::Usage);
        for attach in [
            Attach {
                protocol: VERSION + 1,
                ..Attach::new(80, 24, true)
            },
            Attach::new(0, 24, false),
            Attach::new(80, 0, true),
        ] {
            assert_eq!(attach.validate().unwrap_err().kind, ErrorKind::Usage);
        }
    }

    #[test]
    fn stream_errors_preserve_their_error_kind() {
        let error = MonitorOutput::Error {
            message: "read-only monitor cannot send input".into(),
            error_kind: ErrorKind::Usage,
        };
        let json = serde_json::to_string(&error).unwrap();
        assert!(matches!(
            serde_json::from_str::<MonitorOutput>(&json).unwrap(),
            MonitorOutput::Error {
                error_kind: ErrorKind::Usage,
                ..
            }
        ));
    }
}
