use std::time::Duration;

use super::{bridge, Metadata, Outcome, SessionId};
use crate::{Session, SessionHandle, SessionMonitorTarget, TuiTestError};

/// When completion should leave the terminal available for a first attachment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WaitPolicy {
    #[default]
    Never,
    Failure,
    Always,
}

/// Options for monitoring a session.
#[derive(Debug, Clone)]
pub struct Options {
    pub enabled: bool,
    pub wait_at_end: WaitPolicy,
    /// `None` explicitly permits an infinite wait for the first attachment.
    pub first_attach_timeout: Option<Duration>,
    /// If false, completion may revoke live attachments and close without waiting for clients
    /// to disconnect. The first-attachment timeout is still applied when policy requests it.
    pub hold_while_attached: bool,
    pub metadata: Metadata,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            enabled: false,
            wait_at_end: WaitPolicy::Never,
            first_attach_timeout: Some(Duration::from_secs(30)),
            hold_while_attached: true,
            metadata: Metadata::default(),
        }
    }
}

impl Options {
    pub fn validate(&self) -> Result<(), TuiTestError> {
        if self
            .first_attach_timeout
            .is_some_and(|timeout| std::time::Instant::now().checked_add(timeout).is_none())
        {
            return Err(TuiTestError::usage(
                "monitoring first-attach timeout is too large",
            ));
        }
        Ok(())
    }

    /// Read `TUI_TEST_MONITORING`, `TUI_TEST_WAIT_AT_END`, and
    /// `TUI_TEST_FIRST_ATTACH_TIMEOUT` (milliseconds, or `infinite`).
    /// Invalid values are errors rather than silently disabling inspection.
    pub fn from_env() -> Result<Self, TuiTestError> {
        let mut options = Self::from_values(
            environment("TUI_TEST_MONITORING")?,
            environment("TUI_TEST_WAIT_AT_END")?,
            environment("TUI_TEST_FIRST_ATTACH_TIMEOUT")?,
        )?;
        options.metadata.label = environment("TUI_TEST_LABEL")?;
        Ok(options)
    }

    fn from_values(
        enabled: Option<String>,
        policy: Option<String>,
        timeout: Option<String>,
    ) -> Result<Self, TuiTestError> {
        let wait_at_end = match policy.as_deref().unwrap_or("never") {
            "never" => WaitPolicy::Never,
            "failure" => WaitPolicy::Failure,
            "always" => WaitPolicy::Always,
            _ => {
                return Err(TuiTestError::usage(
                    "TUI_TEST_WAIT_AT_END must be never, failure, or always",
                ))
            }
        };
        let enabled = match enabled.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None => wait_at_end != WaitPolicy::Never,
            Some("1" | "true") => true,
            Some("0" | "false") => false,
            _ => {
                return Err(TuiTestError::usage(
                    "TUI_TEST_MONITORING must be 1, true, 0, or false",
                ))
            }
        };
        let first_attach_timeout = match timeout.as_deref() {
            None => Some(Duration::from_secs(30)),
            Some("infinite") => None,
            Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                Some(Duration::from_millis(value.parse().map_err(|_| {
                    TuiTestError::usage("TUI_TEST_FIRST_ATTACH_TIMEOUT is too large")
                })?))
            }
            _ => {
                return Err(TuiTestError::usage(
                    "TUI_TEST_FIRST_ATTACH_TIMEOUT must be non-negative milliseconds or infinite",
                ))
            }
        };
        let options = Self {
            enabled,
            wait_at_end,
            first_attach_timeout,
            ..Self::default()
        };
        options.validate()?;
        Ok(options)
    }
}

fn environment(name: &str) -> Result<Option<String>, TuiTestError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(TuiTestError::usage(format!(
            "{name} must contain valid Unicode"
        ))),
    }
}

/// Keep an open session available for inspection.
///
/// Call [`finish`](Self::finish) for successful completion, or return the original
/// error through [`finish_failure`](Self::finish_failure). Dropping the guard closes
/// its captured child, treating panic unwinding as failure.
pub struct Monitor {
    name: String,
    target: SessionMonitorTarget,
    handle: Option<SessionHandle>,
    options: Options,
    identity: Option<(SessionId, u64)>,
    finished: bool,
}

impl Monitor {
    pub fn for_session(session: &Session, options: Options) -> Result<Self, TuiTestError> {
        Self::new(session.name(), session.monitor_target(), None, options)
    }

    pub fn for_handle(session: &SessionHandle, options: Options) -> Result<Self, TuiTestError> {
        Self::new(
            session.name(),
            session.monitor_target(),
            Some(session.clone()),
            options,
        )
    }

    fn new(
        name: &str,
        target: Option<SessionMonitorTarget>,
        handle: Option<SessionHandle>,
        options: Options,
    ) -> Result<Self, TuiTestError> {
        options.validate()?;
        let target = target.ok_or_else(TuiTestError::no_session)?;
        let identity = if options.enabled {
            if !bridge::register(name, &target, options.metadata.clone())? {
                return Err(TuiTestError::no_session());
            }
            bridge::target_identity(name, &target)
        } else {
            None
        };
        Ok(Self {
            name: name.into(),
            target,
            handle,
            options,
            identity,
            finished: false,
        })
    }

    /// Session UUID; absent when monitoring is disabled.
    pub fn id(&self) -> Option<SessionId> {
        self.identity.map(|(id, _)| id)
    }

    /// Explicit inspection independent of the automatic completion policy.
    /// Returns whether a long-lived client attached, including a brief attachment.
    pub fn inspect(&self, outcome: Outcome) -> Result<bool, TuiTestError> {
        self.complete(outcome, true)
    }

    fn complete(&self, outcome: Outcome, inspect: bool) -> Result<bool, TuiTestError> {
        if self.identity.is_none() || !self.target.is_current() {
            return Ok(false);
        }
        let timeout = if inspect {
            self.options.first_attach_timeout
        } else {
            Some(Duration::ZERO)
        };
        let (_, generation) = bridge::begin_wait_for_target_with_options(
            &self.name,
            &self.target,
            outcome.as_str(),
            timeout,
            self.options.hold_while_attached,
        )?;
        if inspect {
            self.announce(outcome);
        }
        bridge::wait(
            &self.name,
            generation,
            timeout,
            self.options.hold_while_attached,
        )
    }

    fn announce(&self, outcome: Outcome) {
        eprintln!(
            "[tui-test] {}; terminal kept open for inspection",
            if outcome == Outcome::Failed {
                "Test failed"
            } else {
                "Test completed"
            }
        );
        if let Some(label) = &self.options.metadata.label {
            eprintln!("[tui-test] {label}");
        }
        if let Some(file) = &self.options.metadata.test_file {
            eprintln!("[tui-test] {file}");
        }
        eprintln!(
            "[tui-test] Attach: {}",
            super::host::monitor_command(&self.name, true)
        );
        match self.options.first_attach_timeout {
            Some(timeout) => eprintln!(
                "[tui-test] Waiting up to {}ms for an attachment",
                timeout.as_millis()
            ),
            None => eprintln!("[tui-test] Waiting for an attachment"),
        }
    }

    /// Explicitly abandon inspection, releasing both first-attachment and client-disconnect
    /// waits. This does not close the child; a following finish/close can proceed promptly.
    pub fn cancel_inspection(&self) {
        if let Some((_, generation)) = &self.identity {
            bridge::cancel_wait(&self.name, *generation);
        }
    }

    /// Publish the outcome, apply the inspection policy, and close only this generation.
    /// A monitoring error never prevents the close attempt.
    pub fn finish(&mut self, outcome: Outcome) -> Result<(), TuiTestError> {
        if self.finished {
            return Ok(());
        }
        let should_wait = match self.options.wait_at_end {
            WaitPolicy::Never => false,
            WaitPolicy::Failure => outcome == Outcome::Failed,
            WaitPolicy::Always => true,
        };
        let inspection = self.complete(outcome, should_wait).map(|_| ());
        let closed = match &self.handle {
            Some(handle) => handle.close_target(&self.target),
            None => self.target.close(),
        };
        self.finished = true;
        match (inspection, closed) {
            (Err(error), Err(cleanup)) => {
                eprintln!("[tui-test] terminal cleanup also failed: {cleanup}");
                Err(error)
            }
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        }
    }

    /// Retain the exact original error value; secondary monitoring/cleanup errors
    /// are diagnostics, never replacements for the test failure.
    pub fn finish_failure<E>(&mut self, error: E) -> E {
        if let Err(secondary) = self.finish(Outcome::Failed) {
            eprintln!("tui-test: monitoring cleanup failed: {secondary}");
        }
        error
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        if !self.finished {
            let outcome = if std::thread::panicking() {
                Outcome::Failed
            } else {
                Outcome::Unknown
            };
            if let Err(error) = self.finish(outcome) {
                eprintln!("tui-test: monitoring cleanup failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_is_opt_in_and_strict() {
        assert!(!Options::from_values(None, None, None).unwrap().enabled);
        assert!(
            Options::from_values(None, Some("failure".into()), None)
                .unwrap()
                .enabled
        );
        assert!(
            !Options::from_values(Some("false".into()), Some("always".into()), None)
                .unwrap()
                .enabled
        );
        assert!(Options::from_values(Some("typo".into()), None, None).is_err());
        assert!(Options::from_values(None, Some("typo".into()), None).is_err());
        assert!(Options::from_values(None, None, Some("-1".into())).is_err());
        assert!(Options::from_values(None, None, Some("".into())).is_err());
        assert_eq!(
            Options::from_values(None, None, Some("infinite".into()))
                .unwrap()
                .first_attach_timeout,
            None
        );
    }
}
