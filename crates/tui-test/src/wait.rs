//! Request-scoped quiet waits with per-request cancellation.
//!
//! [`WaitQuietOptions`] selects an overall timeout and a quiet interval for
//! [`Session::wait_quiet`](crate::Session::wait_quiet). Visual-quiet mode
//! samples the raw active screen every 50 milliseconds and restarts its timer
//! when two successive canonical samples differ. Byte-quiet mode restarts its
//! timer on every nonempty successful PTY output read and never samples the
//! screen.
//!
//! The quiet interval starts with the request: each request keeps its previous
//! sample and change time locally, so other callers, viewport movement, and
//! the shared `TermState.last_change` value cannot reset this request's
//! timer. Clocks are monotonic throughout.
//!
//! Sampling limit: visual-quiet mode only sees sampled states. A terminal
//! change that appears and disappears between two consecutive samples is not
//! observed and does not restart the timer. This wait is not an application
//! frame boundary and is not a DEC 2026 boundary.

use std::time::{Duration, Instant};

use crate::api::TuiTestError;
use crate::snapshot::TerminalSnapshot;

/// Default quiet interval for [`WaitQuietOptions`]: 250 milliseconds.
pub const DEFAULT_QUIET_INTERVAL: Duration = Duration::from_millis(250);
/// Default overall timeout for [`WaitQuietOptions`]: 5 seconds.
pub const DEFAULT_QUIET_TIMEOUT: Duration = Duration::from_secs(5);

/// Interval between visual samples during a quiet wait: 50 milliseconds.
pub(crate) const SAMPLE_INTERVAL: Duration = Duration::from_millis(50);

/// Which change class restarts a quiet wait's timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaitQuietMode {
    /// Restart when two successive raw active-screen samples differ.
    #[default]
    Visual,
    /// Restart on every nonempty successful PTY output read.
    Bytes,
}

/// Timeout and quiet-interval selection for a quiet wait.
///
/// Fields are private; validation runs at every construction boundary. The
/// quiet interval must be nonzero, the overall timeout must be nonzero, and
/// the interval must not exceed the timeout. An interval equal to the timeout
/// is accepted but always reports timeout: the deadline and the success
/// instant coincide, and the deadline wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitQuietOptions {
    timeout: Duration,
    quiet: Duration,
    mode: WaitQuietMode,
}

impl WaitQuietOptions {
    /// Build options from an overall timeout and a quiet interval.
    pub fn new(timeout: Duration, quiet: Duration) -> Result<Self, TuiTestError> {
        Self {
            timeout,
            quiet,
            mode: WaitQuietMode::Visual,
        }
        .with_timeout(timeout)
        .and_then(|options| options.with_quiet(quiet))
    }

    /// Overall timeout: the request fails with timeout past this duration.
    pub fn timeout(self) -> Duration {
        self.timeout
    }

    /// Quiet interval: the request succeeds after this duration with no
    /// related change.
    pub fn quiet(self) -> Duration {
        self.quiet
    }

    /// Which change class restarts the timer.
    pub fn mode(self) -> WaitQuietMode {
        self.mode
    }

    /// Set the overall timeout, rejecting a zero value and an interval that
    /// would exceed it.
    pub fn with_timeout(self, timeout: Duration) -> Result<Self, TuiTestError> {
        if timeout.is_zero() {
            return Err(TuiTestError::usage(
                "wait quiet: overall timeout must be nonzero",
            ));
        }
        if self.quiet > timeout {
            return Err(TuiTestError::usage(
                "wait quiet: quiet interval must not exceed the overall timeout",
            ));
        }
        Ok(Self { timeout, ..self })
    }

    /// Set the quiet interval, rejecting a zero value and an interval longer
    /// than the overall timeout.
    pub fn with_quiet(self, quiet: Duration) -> Result<Self, TuiTestError> {
        if quiet.is_zero() {
            return Err(TuiTestError::usage(
                "wait quiet: quiet interval must be nonzero",
            ));
        }
        if quiet > self.timeout {
            return Err(TuiTestError::usage(
                "wait quiet: quiet interval must not exceed the overall timeout",
            ));
        }
        Ok(Self { quiet, ..self })
    }

    /// Set which change class restarts the timer.
    pub fn with_mode(self, mode: WaitQuietMode) -> Self {
        Self { mode, ..self }
    }
}

impl Default for WaitQuietOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_QUIET_TIMEOUT,
            quiet: DEFAULT_QUIET_INTERVAL,
            mode: WaitQuietMode::Visual,
        }
    }
}

/// Canonical visual equality for quiet waits: dimensions, cells, palette,
/// default colors, cursor state, and title.
///
/// Scope, viewport origin, observation metadata, and kept history are
/// excluded on purpose: viewport movement, re-observation, and scrollback
/// growth must not restart a visual-quiet timer. Locator highlighting is
/// excluded because the wait samples raw active-screen data.
pub(crate) fn canonical_visual_eq(first: &TerminalSnapshot, second: &TerminalSnapshot) -> bool {
    first.cols() == second.cols()
        && first.rows() == second.rows()
        && first.cells() == second.cells()
        && first.palette() == second.palette()
        && first.default_foreground() == second.default_foreground()
        && first.default_background() == second.default_background()
        && first.default_cursor() == second.default_cursor()
        && first.cursor() == second.cursor()
        && first.title() == second.title()
}

/// Cancellation result shared with the engine's cancellable waits.
pub(crate) fn cancelled_error() -> TuiTestError {
    TuiTestError::assertion("operation was cancelled")
}

fn timeout_error(options: &WaitQuietOptions) -> TuiTestError {
    TuiTestError::assertion(format!(
        "wait quiet: timed out after {}ms without {}ms of {} quiet",
        options.timeout.as_millis(),
        options.quiet.as_millis(),
        match options.mode {
            WaitQuietMode::Visual => "visual",
            WaitQuietMode::Bytes => "output",
        }
    ))
}

/// Run a quiet wait against caller-supplied observation closures.
///
/// `capture_visual` must capture a raw active-screen snapshot; it is called
/// at request start and then at most every 50 milliseconds in visual mode,
/// and never in byte mode. `last_output` must report the monotonic time of
/// the most recent nonempty successful PTY output read; it is consulted in
/// byte mode only. `cancelled` mirrors
/// [`Engine::execute_with_context_cancellable`](crate::Engine::execute_with_context_cancellable).
///
/// The loop sleep ends at the first of the next poll, the quiet deadline, or
/// the overall deadline. When the quiet and overall deadlines coincide, the
/// overall deadline wins and the request reports timeout.
pub(crate) fn run_quiet_wait(
    options: &WaitQuietOptions,
    cancelled: &dyn Fn() -> bool,
    capture_visual: &dyn Fn() -> Result<TerminalSnapshot, TuiTestError>,
    last_output: &dyn Fn() -> Option<Instant>,
) -> Result<(), TuiTestError> {
    let start = Instant::now();
    let overall_deadline = start
        .checked_add(options.timeout)
        .ok_or_else(|| TuiTestError::internal("wait quiet: overall deadline is out of range"))?;
    if cancelled() {
        return Err(cancelled_error());
    }
    let mut previous = if options.mode == WaitQuietMode::Visual {
        Some(capture_visual()?)
    } else {
        None
    };
    let mut last_change = start;
    if options.mode == WaitQuietMode::Visual {
        // The start capture seeds `previous` once; the first re-sample
        // happens one interval later so captures stay on the 50ms grid
        // instead of sampling twice back-to-back.
        let quiet_deadline = last_change
            .checked_add(options.quiet)
            .ok_or_else(|| TuiTestError::internal("wait quiet: quiet deadline is out of range"))?;
        let first_sleep = [SAMPLE_INTERVAL]
            .into_iter()
            .chain(quiet_deadline.checked_duration_since(start))
            .chain(overall_deadline.checked_duration_since(start))
            .min()
            .unwrap_or(SAMPLE_INTERVAL);
        if !first_sleep.is_zero() {
            std::thread::sleep(first_sleep);
        }
    }
    loop {
        if cancelled() {
            return Err(cancelled_error());
        }
        if Instant::now() >= overall_deadline {
            return Err(timeout_error(options));
        }
        let changed = match options.mode {
            WaitQuietMode::Visual => {
                let sample = capture_visual()?;
                let changed = previous
                    .as_ref()
                    .is_none_or(|before| !canonical_visual_eq(before, &sample));
                previous = Some(sample);
                changed
            }
            WaitQuietMode::Bytes => {
                if let Some(read_at) = last_output() {
                    if read_at > last_change {
                        last_change = read_at;
                    }
                }
                false
            }
        };
        // ONE timestamp after the observation covers both modes: a blocking
        // capture may straddle the overall deadline, so the overall timeout
        // is checked before quiet success and both use this same instant.
        let observed = Instant::now();
        if changed {
            last_change = observed;
        }
        let quiet_deadline = last_change
            .checked_add(options.quiet)
            .ok_or_else(|| TuiTestError::internal("wait quiet: quiet deadline is out of range"))?;
        if observed >= overall_deadline {
            return Err(timeout_error(options));
        }
        if observed >= quiet_deadline {
            // The sample above is the required last sample: it was captured
            // before this success check, so a change observed here restarts
            // the timer instead of reporting success.
            if cancelled() {
                return Err(cancelled_error());
            }
            return Ok(());
        }
        let sleep = [SAMPLE_INTERVAL]
            .into_iter()
            .chain(quiet_deadline.checked_duration_since(observed))
            .chain(overall_deadline.checked_duration_since(observed))
            .min()
            .unwrap_or(SAMPLE_INTERVAL);
        if !sleep.is_zero() {
            std::thread::sleep(sleep);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Rgb;
    use crate::snapshot::{
        SnapshotCell, SnapshotColor, SnapshotCursor, SnapshotCursorShape, SnapshotMetadata,
        SnapshotParts, SnapshotScope, SnapshotUnderline,
    };
    use crate::terminal::cell::Attrs;

    fn test_parts() -> SnapshotParts {
        let palette = [Rgb::new(1, 2, 3); 256];
        let cell = SnapshotCell::new(
            "x".to_string(),
            1,
            SnapshotColor::Default,
            SnapshotColor::Default,
            Attrs::empty(),
            SnapshotUnderline::None,
            SnapshotColor::Default,
            None,
            None,
        )
        .expect("test cell is valid");
        SnapshotParts {
            cols: 2,
            rows: 1,
            scope: SnapshotScope::Viewport,
            viewport_origin: 0,
            cells: vec![
                cell.clone(),
                SnapshotCell::new(
                    "y".to_string(),
                    1,
                    SnapshotColor::Default,
                    SnapshotColor::Default,
                    Attrs::empty(),
                    SnapshotUnderline::None,
                    SnapshotColor::Default,
                    None,
                    None,
                )
                .expect("test cell is valid"),
            ],
            palette,
            default_foreground: Rgb::new(10, 10, 10),
            default_background: Rgb::new(20, 20, 20),
            default_cursor: Rgb::new(30, 30, 30),
            cursor: SnapshotCursor::new(
                0,
                0,
                false,
                true,
                SnapshotCursorShape::Block,
                Rgb::new(30, 30, 30),
                None,
            ),
            title: Some("title".to_string()),
            metadata: SnapshotMetadata::default(),
        }
    }

    fn snapshot() -> TerminalSnapshot {
        TerminalSnapshot::try_new(test_parts()).expect("test snapshot is valid")
    }

    #[test]
    fn canonical_visual_eq_ignores_scope_origin_and_metadata() {
        let first = snapshot();
        let mut parts = test_parts();
        parts.scope = SnapshotScope::ActiveScreen;
        parts.viewport_origin = 42;
        parts.metadata.captured_at_ms = 999;
        parts.metadata.revision = 42;
        parts.metadata.backend = "other".to_string();
        let second = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        assert!(canonical_visual_eq(&first, &second));
    }

    #[test]
    fn canonical_visual_eq_detects_palette_only_change() {
        let first = snapshot();
        let mut parts = test_parts();
        parts.palette[200] = Rgb::new(9, 9, 9);
        let second = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        assert!(!canonical_visual_eq(&first, &second));
    }

    #[test]
    fn canonical_visual_eq_detects_title_only_change() {
        let first = snapshot();
        let mut parts = test_parts();
        parts.title = Some("other".to_string());
        let second = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        assert!(!canonical_visual_eq(&first, &second));
    }

    #[test]
    fn canonical_visual_eq_detects_cursor_only_change() {
        let first = snapshot();
        let mut parts = test_parts();
        parts.cursor = SnapshotCursor::new(
            1,
            0,
            false,
            true,
            SnapshotCursorShape::Block,
            Rgb::new(30, 30, 30),
            None,
        );
        let second = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        assert!(!canonical_visual_eq(&first, &second));
    }

    #[test]
    fn canonical_visual_eq_detects_dimension_only_change() {
        let first = snapshot();
        let mut parts = test_parts();
        parts.cols = 3;
        let extra = parts.cells[0].clone();
        parts.cells.push(extra);
        let second = TerminalSnapshot::try_new(parts).expect("test snapshot is valid");
        assert!(!canonical_visual_eq(&first, &second));
    }

    #[test]
    fn options_reject_zero_and_overlong_intervals() {
        assert!(WaitQuietOptions::new(Duration::ZERO, Duration::from_millis(1)).is_err());
        assert!(WaitQuietOptions::new(Duration::from_secs(1), Duration::ZERO).is_err());
        assert!(
            WaitQuietOptions::new(Duration::from_millis(100), Duration::from_millis(200)).is_err()
        );
        assert!(
            WaitQuietOptions::new(Duration::from_millis(200), Duration::from_millis(200)).is_ok()
        );
        assert_eq!(WaitQuietOptions::default().quiet(), DEFAULT_QUIET_INTERVAL);
        assert_eq!(WaitQuietOptions::default().timeout(), DEFAULT_QUIET_TIMEOUT);
    }

    #[test]
    fn unseen_transient_between_samples_does_not_restart_visual_timer() {
        // Sampling-limit demonstration with controlled samples: a terminal
        // change appears and disappears between two consecutive polls, so the
        // sampler only ever observes the stable screen. The wait succeeds
        // after one quiet interval even though the terminal changed.
        let stable = snapshot();
        let capture = || Ok(stable.clone());
        let options = WaitQuietOptions::new(Duration::from_secs(5), Duration::from_millis(100))
            .expect("test options are valid");
        let started = Instant::now();
        run_quiet_wait(&options, &|| false, &capture, &|| None)
            .expect("unseen transient must not delay success");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
