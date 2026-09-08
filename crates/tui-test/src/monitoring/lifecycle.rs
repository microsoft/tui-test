use std::time::{Duration, Instant};

use crate::TuiTestError;

/// Test completion, independent of whether the child process has exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Passed,
    Failed,
    Cancelled,
    Unknown,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, TuiTestError> {
        match value {
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "unknown" => Ok(Self::Unknown),
            _ => Err(TuiTestError::usage(
                "monitoring outcome must be passed, failed, cancelled, or unknown",
            )),
        }
    }
}

#[derive(Clone)]
pub(super) struct Hold {
    baseline: u64,
    had_attachment: bool,
    pub deadline: Option<Instant>,
    hold_while_attached: bool,
}

impl Hold {
    pub fn new(
        epoch: u64,
        clients: usize,
        timeout: Option<Duration>,
        hold_while_attached: bool,
    ) -> Result<Self, TuiTestError> {
        Ok(Self {
            baseline: epoch,
            had_attachment: clients != 0,
            deadline: deadline(timeout)?,
            hold_while_attached,
        })
    }

    pub fn configure(&mut self, timeout: Option<Duration>, hold: bool) -> Result<(), TuiTestError> {
        self.deadline = deadline(timeout)?;
        self.hold_while_attached = hold;
        Ok(())
    }

    pub fn observed(&self, epoch: u64, clients: usize) -> bool {
        self.had_attachment || clients != 0 || epoch != self.baseline
    }

    pub fn completed(&self, epoch: u64, clients: usize, now: Instant) -> Option<bool> {
        let observed = self.observed(epoch, clients);
        if observed {
            (!self.hold_while_attached || clients == 0).then_some(true)
        } else {
            self.deadline
                .filter(|deadline| now >= *deadline)
                .map(|_| false)
        }
    }
}

fn deadline(timeout: Option<Duration>) -> Result<Option<Instant>, TuiTestError> {
    timeout
        .map(|timeout| {
            Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| TuiTestError::usage("monitoring first-attach timeout is too large"))
        })
        .transpose()
}

#[derive(Clone)]
pub(super) enum Lifecycle {
    Running,
    Holding(Hold),
    Completed {
        attached: bool,
        hold_while_attached: bool,
    },
    Closing {
        attached: bool,
        hold_while_attached: bool,
    },
}

impl Lifecycle {
    pub fn accepts_attachments(&self) -> bool {
        matches!(self, Self::Running | Self::Holding(_))
    }

    pub fn attached(&self, epoch: u64, clients: usize) -> bool {
        match self {
            Self::Running => clients != 0,
            Self::Holding(hold) => hold.observed(epoch, clients),
            Self::Completed { attached, .. } | Self::Closing { attached, .. } => *attached,
        }
    }

    pub fn hold_while_attached(&self) -> bool {
        match self {
            Self::Running => true,
            Self::Holding(hold) => hold.hold_while_attached,
            Self::Completed {
                hold_while_attached,
                ..
            }
            | Self::Closing {
                hold_while_attached,
                ..
            } => *hold_while_attached,
        }
    }

    pub fn advance(&mut self, epoch: u64, clients: usize) -> bool {
        if let Self::Holding(hold) = self {
            if let Some(attached) = hold.completed(epoch, clients, Instant::now()) {
                *self = Self::Completed {
                    attached,
                    hold_while_attached: hold.hold_while_attached,
                };
                return true;
            }
        }
        false
    }

    pub fn status(&self, clients: usize, outcome: Option<Outcome>) -> String {
        match self {
            Self::Running if clients == 0 => "running".into(),
            Self::Running => "attached".into(),
            Self::Holding(_) if clients == 0 => "waiting-for-attach".into(),
            Self::Holding(_) => "attached".into(),
            Self::Completed { .. } => {
                format!("completed-{}", outcome.unwrap_or(Outcome::Unknown).as_str())
            }
            Self::Closing { .. } => "closing".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_existing_and_brief_attachments() {
        for (baseline, clients, epoch) in [(1, 1, 1), (0, 0, 1)] {
            let hold = Hold::new(baseline, clients, None, true).unwrap();
            assert_eq!(hold.completed(epoch, 0, Instant::now()), Some(true));
        }
    }

    #[test]
    fn first_attach_timeout_does_not_expire_attached_hold() {
        let hold = Hold::new(0, 0, Some(Duration::ZERO), true).unwrap();
        assert_eq!(hold.completed(0, 0, Instant::now()), Some(false));
        assert_eq!(hold.completed(1, 1, Instant::now()), None);
        assert_eq!(hold.completed(1, 0, Instant::now()), Some(true));
    }

    #[test]
    fn infinite_wait_and_nonholding_inspection_are_explicit() {
        let hold = Hold::new(0, 0, None, false).unwrap();
        assert_eq!(hold.completed(0, 0, Instant::now()), None);
        assert_eq!(hold.completed(1, 1, Instant::now()), Some(true));
    }

    #[test]
    fn completed_and_closing_reject_new_attachments() {
        assert!(Lifecycle::Running.accepts_attachments());
        assert!(!Lifecycle::Completed {
            attached: false,
            hold_while_attached: true
        }
        .accepts_attachments());
        assert!(!Lifecycle::Closing {
            attached: false,
            hold_while_attached: true
        }
        .accepts_attachments());
    }

    #[test]
    fn completed_wait_retains_its_explicit_close_policy() {
        let mut lifecycle = Lifecycle::Holding(Hold::new(0, 1, None, false).unwrap());
        assert!(lifecycle.advance(1, 1));
        assert!(!lifecycle.hold_while_attached());
        assert!(Lifecycle::Running.hold_while_attached());
    }
}
