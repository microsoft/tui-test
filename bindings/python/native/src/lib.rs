use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyInt, PyList, PyMemoryView, PyModule, PyTuple};
use tui_test::profile::{Profile as CoreProfile, Rgb};
use tui_test::runtime::global_registry;
use tui_test::shell::Shell;
use tui_test::{
    AutomaticRecording as CoreAutomaticRecording,
    AutomaticRecordingMode as CoreAutomaticRecordingMode, Backend, BellEvent, CaptureBackground,
    Cell, CellColor, ClipboardPattern, Cursor, ErrorKind, KeyAction, LocatorDirection,
    LocatorQuery, LocatorSelector, MatchOccurrence, MouseAction, MouseOptions, OpenOptions,
    OpenResult, Operation, OperationResult, PackedScreen, RecordingFormat, RunOptions,
    ScreenshotResult, Size, SnapshotResult, State, StyleSelector, TextMatch, TextSelector,
    TextStyle, Timeouts, TuiTestError, WhitespaceMode,
};

pyo3::create_exception!(
    tui_test._native,
    NativeAssertionError,
    PyException,
    "Native assertion failure."
);
pyo3::create_exception!(
    tui_test._native,
    NativeUsageError,
    PyException,
    "Native usage error."
);
pyo3::create_exception!(
    tui_test._native,
    NativeNoSessionError,
    PyException,
    "Native session was not found."
);
pyo3::create_exception!(
    tui_test._native,
    NativeInternalError,
    PyException,
    "Native internal error."
);

#[pyclass(module = "tui_test._native", frozen)]
struct NativeSession {
    #[pyo3(get)]
    name: String,
    recording: CoreAutomaticRecording,
}

#[pymethods]
impl NativeSession {
    #[new]
    #[pyo3(signature = (name, recording_mode = None, recording_directory = None))]
    fn new(
        name: String,
        recording_mode: Option<String>,
        recording_directory: Option<String>,
    ) -> PyResult<Self> {
        let mode = match recording_mode.as_deref().unwrap_or("always") {
            "disabled" => CoreAutomaticRecordingMode::Disabled,
            "on-failure" => CoreAutomaticRecordingMode::OnFailure,
            "always" => CoreAutomaticRecordingMode::Always,
            other => {
                return Err(shell_error_to_py(TuiTestError::usage(format!(
                    "unknown automatic recording mode {other:?}; expected disabled, on-failure, or always"
                ))))
            }
        };
        Ok(Self {
            name,
            recording: CoreAutomaticRecording {
                mode,
                directory: recording_directory.map(Into::into),
            },
        })
    }

    #[pyo3(signature = (
        shell,
        backend,
        cols,
        rows,
        cwd,
        env,
        wait_ready,
        restart,
        profile_scrollback,
        profile_colors,
        text_timeout,
        idle_timeout,
        command_timeout,
        exit_timeout,
        ready_timeout
    ))]
    #[allow(clippy::too_many_arguments)]
    fn open<'py>(
        &self,
        py: Python<'py>,
        shell: Option<String>,
        backend: Option<String>,
        cols: Bound<'py, PyAny>,
        rows: Bound<'py, PyAny>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        wait_ready: Option<bool>,
        restart: bool,
        profile_scrollback: Option<Bound<'py, PyAny>>,
        profile_colors: Vec<(String, String)>,
        text_timeout: Option<Bound<'py, PyAny>>,
        idle_timeout: Option<Bound<'py, PyAny>>,
        command_timeout: Option<Bound<'py, PyAny>>,
        exit_timeout: Option<Bound<'py, PyAny>>,
        ready_timeout: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let cols = capture_integer(&cols);
        let rows = capture_integer(&rows);
        let profile_scrollback = capture_optional_integer(profile_scrollback);
        let text_timeout = capture_optional_integer(text_timeout);
        let idle_timeout = capture_optional_integer(idle_timeout);
        let command_timeout = capture_optional_integer(command_timeout);
        let exit_timeout = capture_optional_integer(exit_timeout);
        let ready_timeout = capture_optional_integer(ready_timeout);
        let name = self.name.clone();
        let recording = self.recording.clone();
        future_blocking(
            py,
            move || {
                execute_open(
                    &name,
                    Operation::Open(OpenOptions {
                        backend: parse_backend(backend.as_deref())?,
                        profile: profile_from_parts(profile_scrollback.as_ref(), &profile_colors)?,
                        shell: parse_shell(shell.as_deref())?,
                        cols: integer_u16(&cols, "cols")?,
                        rows: integer_u16(&rows, "rows")?,
                        cwd,
                        env,
                        wait_ready,
                        restart,
                        timeouts: Timeouts {
                            text: optional_u64(text_timeout.as_ref(), "text_timeout")?,
                            idle: optional_u64(idle_timeout.as_ref(), "idle_timeout")?,
                            command: optional_u64(command_timeout.as_ref(), "command_timeout")?,
                            exit: optional_u64(exit_timeout.as_ref(), "exit_timeout")?,
                            ready: optional_u64(ready_timeout.as_ref(), "ready_timeout")?,
                        },
                        recording,
                    }),
                )
            },
            open_to_py,
        )
    }

    #[pyo3(signature = (
        program,
        args,
        backend,
        cols,
        rows,
        cwd,
        env,
        wait_ready,
        restart,
        profile_scrollback,
        profile_colors,
        text_timeout,
        idle_timeout,
        command_timeout,
        exit_timeout,
        ready_timeout
    ))]
    #[allow(clippy::too_many_arguments)]
    fn run<'py>(
        &self,
        py: Python<'py>,
        program: String,
        args: Vec<String>,
        backend: Option<String>,
        cols: Bound<'py, PyAny>,
        rows: Bound<'py, PyAny>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        wait_ready: Option<bool>,
        restart: bool,
        profile_scrollback: Option<Bound<'py, PyAny>>,
        profile_colors: Vec<(String, String)>,
        text_timeout: Option<Bound<'py, PyAny>>,
        idle_timeout: Option<Bound<'py, PyAny>>,
        command_timeout: Option<Bound<'py, PyAny>>,
        exit_timeout: Option<Bound<'py, PyAny>>,
        ready_timeout: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let cols = capture_integer(&cols);
        let rows = capture_integer(&rows);
        let profile_scrollback = capture_optional_integer(profile_scrollback);
        let text_timeout = capture_optional_integer(text_timeout);
        let idle_timeout = capture_optional_integer(idle_timeout);
        let command_timeout = capture_optional_integer(command_timeout);
        let exit_timeout = capture_optional_integer(exit_timeout);
        let ready_timeout = capture_optional_integer(ready_timeout);
        let name = self.name.clone();
        let recording = self.recording.clone();
        future_blocking(
            py,
            move || {
                execute_open(
                    &name,
                    Operation::Run(RunOptions {
                        backend: parse_backend(backend.as_deref())?,
                        profile: profile_from_parts(profile_scrollback.as_ref(), &profile_colors)?,
                        program,
                        args,
                        cols: integer_u16(&cols, "cols")?,
                        rows: integer_u16(&rows, "rows")?,
                        cwd,
                        env,
                        wait_ready,
                        restart,
                        timeouts: Timeouts {
                            text: optional_u64(text_timeout.as_ref(), "text_timeout")?,
                            idle: optional_u64(idle_timeout.as_ref(), "idle_timeout")?,
                            command: optional_u64(command_timeout.as_ref(), "command_timeout")?,
                            exit: optional_u64(exit_timeout.as_ref(), "exit_timeout")?,
                            ready: optional_u64(ready_timeout.as_ref(), "ready_timeout")?,
                        },
                        recording,
                    }),
                )
            },
            open_to_py,
        )
    }

    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_unit(&name, Operation::Close),
            unit_to_py,
        )
    }

    fn state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_state(&name, Operation::State),
            state_to_py,
        )
    }

    fn text<'py>(&self, py: Python<'py>, full: bool) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_text(&name, Operation::Text { full }),
            string_to_py,
        )
    }

    fn find_locator<'py>(
        &self,
        py: Python<'py>,
        stages: Vec<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let query = capture_locator_query(&stages);
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_matches(&name, Operation::FindLocator { query: query? }),
            matches_to_py,
        )
    }

    #[pyo3(signature = (stages, not_, timeout_ms))]
    fn wait_locator<'py>(
        &self,
        py: Python<'py>,
        stages: Vec<Bound<'py, PyAny>>,
        not_: bool,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let query = capture_locator_query(&stages);
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::WaitLocator {
                        query: query?,
                        not: not_,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (stages, button, clicks, timeout_ms))]
    fn click_locator<'py>(
        &self,
        py: Python<'py>,
        stages: Vec<Bound<'py, PyAny>>,
        button: Bound<'py, PyAny>,
        clicks: Bound<'py, PyAny>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let query = capture_locator_query(&stages);
        let button = capture_integer(&button);
        let clicks = capture_integer(&clicks);
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::ClickLocator {
                        query: query?,
                        options: mouse_options(&button)?,
                        clicks: integer_u8(&clicks, "clicks")?,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (stages, timeout_ms))]
    fn highlight_locator<'py>(
        &self,
        py: Python<'py>,
        stages: Vec<Bound<'py, PyAny>>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let query = capture_locator_query(&stages);
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_matches(
                    &name,
                    Operation::HighlightLocator {
                        query: query?,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            matches_to_py,
        )
    }

    #[pyo3(signature = (stages, not_, timeout_ms))]
    fn expect_locator<'py>(
        &self,
        py: Python<'py>,
        stages: Vec<Bound<'py, PyAny>>,
        not_: bool,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let query = capture_locator_query(&stages);
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::WaitLocator {
                        query: query?,
                        not: not_,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    fn packed_screen<'py>(&self, py: Python<'py>, full: bool) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_packed_screen(&name, Operation::PackedScreen { full }),
            packed_screen_to_py,
        )
    }

    fn cells<'py>(
        &self,
        py: Python<'py>,
        x: Bound<'py, PyAny>,
        y: Bound<'py, PyAny>,
        w: Bound<'py, PyAny>,
        h: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let x = capture_integer(&x);
        let y = capture_integer(&y);
        let w = capture_integer(&w);
        let h = capture_integer(&h);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_cells(
                    &name,
                    Operation::Cells {
                        x: integer_u16(&x, "x")?,
                        y: integer_u16(&y, "y")?,
                        w: integer_u16(&w, "w")?,
                        h: integer_u16(&h, "h")?,
                    },
                )
            },
            cells_to_py,
        )
    }

    fn get_command<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_command(&name, Operation::GetCommand),
            optional_string_to_py,
        )
    }

    fn get_output<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_output(&name, Operation::GetOutput),
            optional_string_to_py,
        )
    }

    fn get_exit_code<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_exit_code(&name, Operation::GetExitCode),
            optional_i32_to_py,
        )
    }

    fn get_cwd<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_cwd(&name, Operation::GetCwd),
            optional_string_to_py,
        )
    }

    fn get_title<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_title(&name, Operation::GetTitle),
            optional_string_to_py,
        )
    }

    fn get_clipboard<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_clipboard(&name, Operation::GetClipboard),
            string_to_py,
        )
    }

    fn get_cursor<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_cursor(&name, Operation::GetCursor),
            cursor_to_py,
        )
    }

    fn get_size<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_size(&name, Operation::GetSize),
            size_to_py,
        )
    }

    fn get_bell_count<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_bell_count(&name, Operation::GetBellCount),
            u64_to_py,
        )
    }

    fn get_bell_events<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_bell_events(&name, Operation::GetBellEvents),
            bell_events_to_py_object,
        )
    }

    fn write<'py>(&self, py: Python<'py>, data: String) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_unit(&name, Operation::Write { data }),
            unit_to_py,
        )
    }

    #[pyo3(name = "type")]
    fn type_text<'py>(&self, py: Python<'py>, text: String) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_unit(&name, Operation::Write { data: text }),
            unit_to_py,
        )
    }

    #[pyo3(signature = (data))]
    fn submit<'py>(&self, py: Python<'py>, data: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_unit(&name, Operation::Submit { data }),
            unit_to_py,
        )
    }

    fn press<'py>(&self, py: Python<'py>, keys: Vec<String>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Key {
                        keys,
                        action: KeyAction::Press,
                    },
                )
            },
            unit_to_py,
        )
    }

    fn key_down<'py>(&self, py: Python<'py>, keys: Vec<String>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Key {
                        keys,
                        action: KeyAction::Down,
                    },
                )
            },
            unit_to_py,
        )
    }

    fn repeat<'py>(&self, py: Python<'py>, keys: Vec<String>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Key {
                        keys,
                        action: KeyAction::Repeat,
                    },
                )
            },
            unit_to_py,
        )
    }

    fn key_up<'py>(&self, py: Python<'py>, keys: Vec<String>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Key {
                        keys,
                        action: KeyAction::Up,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (x, y, on_text, button, clicks))]
    fn mouse_click<'py>(
        &self,
        py: Python<'py>,
        x: Option<Bound<'py, PyAny>>,
        y: Option<Bound<'py, PyAny>>,
        on_text: Option<String>,
        button: Bound<'py, PyAny>,
        clicks: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let x = capture_optional_integer(x);
        let y = capture_optional_integer(y);
        let button = capture_integer(&button);
        let clicks = capture_integer(&clicks);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Mouse {
                        action: MouseAction::Click {
                            x: x.as_ref().map(|x| integer_u16(x, "x")).transpose()?,
                            y: y.as_ref().map(|y| integer_u16(y, "y")).transpose()?,
                            on_text,
                            options: mouse_options(&button)?,
                            clicks: integer_u8(&clicks, "clicks")?,
                        },
                    },
                )
            },
            unit_to_py,
        )
    }

    fn mouse_move<'py>(
        &self,
        py: Python<'py>,
        x: Bound<'py, PyAny>,
        y: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let x = capture_integer(&x);
        let y = capture_integer(&y);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Mouse {
                        action: MouseAction::Move {
                            x: integer_u16(&x, "x")?,
                            y: integer_u16(&y, "y")?,
                        },
                    },
                )
            },
            unit_to_py,
        )
    }

    fn mouse_down<'py>(
        &self,
        py: Python<'py>,
        x: Bound<'py, PyAny>,
        y: Bound<'py, PyAny>,
        button: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let x = capture_integer(&x);
        let y = capture_integer(&y);
        let button = capture_integer(&button);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Mouse {
                        action: MouseAction::Down {
                            x: integer_u16(&x, "x")?,
                            y: integer_u16(&y, "y")?,
                            options: mouse_options(&button)?,
                        },
                    },
                )
            },
            unit_to_py,
        )
    }

    fn mouse_up<'py>(
        &self,
        py: Python<'py>,
        x: Bound<'py, PyAny>,
        y: Bound<'py, PyAny>,
        button: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let x = capture_integer(&x);
        let y = capture_integer(&y);
        let button = capture_integer(&button);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Mouse {
                        action: MouseAction::Up {
                            x: integer_u16(&x, "x")?,
                            y: integer_u16(&y, "y")?,
                            options: mouse_options(&button)?,
                        },
                    },
                )
            },
            unit_to_py,
        )
    }

    fn mouse_drag<'py>(
        &self,
        py: Python<'py>,
        x1: Bound<'py, PyAny>,
        y1: Bound<'py, PyAny>,
        x2: Bound<'py, PyAny>,
        y2: Bound<'py, PyAny>,
        button: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let x1 = capture_integer(&x1);
        let y1 = capture_integer(&y1);
        let x2 = capture_integer(&x2);
        let y2 = capture_integer(&y2);
        let button = capture_integer(&button);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Mouse {
                        action: MouseAction::Drag {
                            x1: integer_u16(&x1, "x1")?,
                            y1: integer_u16(&y1, "y1")?,
                            x2: integer_u16(&x2, "x2")?,
                            y2: integer_u16(&y2, "y2")?,
                            options: mouse_options(&button)?,
                        },
                    },
                )
            },
            unit_to_py,
        )
    }

    fn mouse_scroll<'py>(
        &self,
        py: Python<'py>,
        direction: String,
        amount: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let amount = capture_integer(&amount);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Mouse {
                        action: MouseAction::Scroll {
                            direction,
                            amount: integer_u16(&amount, "amount")?,
                        },
                    },
                )
            },
            unit_to_py,
        )
    }

    fn resize<'py>(
        &self,
        py: Python<'py>,
        cols: Bound<'py, PyAny>,
        rows: Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let cols = capture_integer(&cols);
        let rows = capture_integer(&rows);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Resize {
                        cols: integer_u16(&cols, "cols")?,
                        rows: integer_u16(&rows, "rows")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    fn signal<'py>(&self, py: Python<'py>, signal: String) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_unit(&name, Operation::Signal { name: signal }),
            unit_to_py,
        )
    }

    fn kill<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::Signal {
                        name: "KILL".to_string(),
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (text, regex, not_, timeout_ms))]
    fn wait_title<'py>(
        &self,
        py: Python<'py>,
        text: String,
        regex: bool,
        not_: bool,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::WaitTitle {
                        text,
                        regex,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                        not: not_,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (text, regex, timeout_ms))]
    fn wait_clipboard<'py>(
        &self,
        py: Python<'py>,
        text: Option<String>,
        regex: bool,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                let operation = match text {
                    Some(text) => {
                        let pattern = if regex {
                            ClipboardPattern::regex(&text).map_err(|error| {
                                TuiTestError::usage(format!("invalid regex: {error}"))
                            })?
                        } else {
                            text.into()
                        };
                        Operation::WaitClipboardMatch {
                            pattern,
                            timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                        }
                    }
                    None if regex => {
                        return Err(TuiTestError::usage("clipboard regex requires text"))
                    }
                    None => Operation::WaitClipboard {
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                };
                execute_unit(&name, operation)
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (timeout_ms))]
    fn wait_idle<'py>(
        &self,
        py: Python<'py>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::WaitIdle {
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (timeout_ms))]
    fn wait_command<'py>(
        &self,
        py: Python<'py>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::WaitCommand {
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (timeout_ms))]
    fn wait_exit<'py>(
        &self,
        py: Python<'py>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::WaitExit {
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (timeout_ms))]
    fn wait_ready<'py>(
        &self,
        py: Python<'py>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::WaitReady {
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (timeout_ms))]
    fn wait_bell<'py>(
        &self,
        py: Python<'py>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::WaitBell {
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (text, regex, not_, timeout_ms))]
    fn expect_title<'py>(
        &self,
        py: Python<'py>,
        text: String,
        regex: bool,
        not_: bool,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::ExpectTitle {
                        text,
                        regex,
                        not: not_,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (code, timeout_ms))]
    fn expect_exit_code<'py>(
        &self,
        py: Python<'py>,
        code: Bound<'py, PyAny>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let code = capture_integer(&code);
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::ExpectExitCode {
                        code: integer_i32(&code, "code")?,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    fn expect_output<'py>(
        &self,
        py: Python<'py>,
        text: String,
        regex: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_unit(&name, Operation::ExpectOutput { text, regex }),
            unit_to_py,
        )
    }

    #[pyo3(signature = (count, timeout_ms))]
    fn expect_bell_count<'py>(
        &self,
        py: Python<'py>,
        count: Bound<'py, PyAny>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let count = capture_integer(&count);
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::ExpectBellCount {
                        count: integer_u64(&count, "count")?,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                    },
                )
            },
            unit_to_py,
        )
    }

    #[pyo3(signature = (name, update, include_colors, include_title, cwd))]
    fn snapshot<'py>(
        &self,
        py: Python<'py>,
        name: String,
        update: bool,
        include_colors: bool,
        include_title: bool,
        cwd: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let session = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_snapshot(
                    &session,
                    Operation::Snapshot {
                        name,
                        update,
                        include_colors,
                        include_title,
                        cwd,
                    },
                )
            },
            snapshot_to_py,
        )
    }

    #[pyo3(signature = (path, full, zoom=None, background=None, transparent=false))]
    fn screenshot<'py>(
        &self,
        py: Python<'py>,
        path: Option<String>,
        full: bool,
        zoom: Option<f64>,
        background: Option<String>,
        transparent: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let background = capture_background(background, transparent).map_err(shell_error_to_py)?;
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_screenshot(
                    &name,
                    Operation::Screenshot {
                        full,
                        path,
                        zoom,
                        background,
                    },
                )
            },
            screenshot_to_py,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (path, format, fps, speed, idle_time_limit, zoom=None, background=None, transparent=false))]
    fn start_recording<'py>(
        &self,
        py: Python<'py>,
        path: String,
        format: Option<String>,
        fps: Option<Bound<'py, PyAny>>,
        speed: Option<f64>,
        idle_time_limit: Option<f64>,
        zoom: Option<f64>,
        background: Option<String>,
        transparent: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let fps = capture_optional_integer(fps);
        let background = capture_background(background, transparent).map_err(shell_error_to_py)?;
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::StartRecording {
                        path,
                        format: parse_recording_format(format.as_deref())?,
                        fps: fps
                            .as_ref()
                            .map(|value| integer_u8(value, "fps"))
                            .transpose()?,
                        speed,
                        idle_time_limit,
                        zoom,
                        background,
                    },
                )
            },
            unit_to_py,
        )
    }

    fn stop_recording<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_recording(&name, Operation::StopRecording),
            string_to_py,
        )
    }

    fn recording<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                global_registry()
                    .recording(&name)
                    .map_err(io_error_to_shell_error)
            },
            string_to_py,
        )
    }
}

#[pyfunction]
fn sessions(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    future_blocking(py, || Ok(global_registry().sessions()), string_list_to_py)
}

#[pyfunction]
fn close_all(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    future_blocking(
        py,
        || {
            global_registry().close_all();
            Ok(())
        },
        unit_to_py,
    )
}

#[pyfunction]
fn recording(py: Python<'_>, name: String) -> PyResult<Bound<'_, PyAny>> {
    future_blocking(
        py,
        move || {
            global_registry()
                .recording(&name)
                .map_err(io_error_to_shell_error)
        },
        string_to_py,
    )
}

#[pyfunction]
fn panic_probe(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    future_blocking(
        py,
        || -> Result<(), TuiTestError> {
            panic!("tui-test Python panic probe");
        },
        unit_to_py,
    )
}

#[pyfunction]
fn _close_all_blocking(py: Python<'_>) {
    py.detach(|| global_registry().close_all());
}

fn future_blocking<'py, T, F>(
    py: Python<'py>,
    task: F,
    convert: for<'a> fn(Python<'a>, T) -> PyResult<Py<PyAny>>,
) -> PyResult<Bound<'py, PyAny>>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, TuiTestError> + Send + 'static,
{
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let value = run_blocking(task).await.map_err(shell_error_to_py)?;
        Python::attach(|py| convert(py, value))
    })
}

async fn run_blocking<T, F>(task: F) -> Result<T, TuiTestError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, TuiTestError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        catch_unwind(AssertUnwindSafe(task)).unwrap_or_else(|payload| {
            Err(TuiTestError::internal(format!(
                "native Python operation panicked: {}",
                panic_message(payload.as_ref())
            )))
        })
    })
    .await
    .map_err(|error| TuiTestError::internal(format!("native Python worker failed: {error}")))?
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

fn shell_error_to_py(error: TuiTestError) -> PyErr {
    Python::attach(|py| {
        let exception = match error.kind {
            ErrorKind::Assertion => py.get_type::<NativeAssertionError>(),
            ErrorKind::Usage => py.get_type::<NativeUsageError>(),
            ErrorKind::NoSession => py.get_type::<NativeNoSessionError>(),
            ErrorKind::Internal => py.get_type::<NativeInternalError>(),
        };
        PyErr::from_type(exception, error.message)
    })
}

fn io_error_to_shell_error(error: std::io::Error) -> TuiTestError {
    if error.kind() == std::io::ErrorKind::NotFound {
        TuiTestError::new(ErrorKind::NoSession, error.to_string())
    } else {
        TuiTestError::internal(error.to_string())
    }
}

#[derive(Clone)]
enum IntegerInput {
    Negative(i64),
    NonNegative(u64),
    Invalid,
}

fn capture_integer(value: &Bound<'_, PyAny>) -> IntegerInput {
    if value.is_instance_of::<PyBool>() {
        return IntegerInput::Invalid;
    }

    let Ok(index) = value.call_method0("__index__") else {
        return IntegerInput::Invalid;
    };
    if index.is_instance_of::<PyBool>() || !index.is_instance_of::<PyInt>() {
        return IntegerInput::Invalid;
    }

    if let Ok(value) = index.extract::<i64>() {
        return if value < 0 {
            IntegerInput::Negative(value)
        } else {
            IntegerInput::NonNegative(value as u64)
        };
    }
    index
        .extract::<u64>()
        .map(IntegerInput::NonNegative)
        .unwrap_or(IntegerInput::Invalid)
}

fn capture_optional_integer(value: Option<Bound<'_, PyAny>>) -> Option<IntegerInput> {
    value.as_ref().map(capture_integer)
}

fn integer_u8(value: &IntegerInput, name: &str) -> Result<u8, TuiTestError> {
    integer_unsigned(value, name, u8::MAX as u128).map(|value| value as u8)
}

fn mouse_options(value: &IntegerInput) -> Result<MouseOptions, TuiTestError> {
    let code = integer_u8(value, "button")?;
    MouseOptions::from_sgr_code(code)
        .ok_or_else(|| TuiTestError::usage(format!("invalid mouse button code {code}")))
}

fn integer_u16(value: &IntegerInput, name: &str) -> Result<u16, TuiTestError> {
    integer_unsigned(value, name, u16::MAX as u128).map(|value| value as u16)
}

fn integer_u64(value: &IntegerInput, name: &str) -> Result<u64, TuiTestError> {
    integer_unsigned(value, name, u64::MAX as u128).map(|value| value as u64)
}

fn optional_u64(value: Option<&IntegerInput>, name: &str) -> Result<Option<u64>, TuiTestError> {
    value.map(|value| integer_u64(value, name)).transpose()
}

fn py_item<'py>(
    dict: &Bound<'py, PyDict>,
    key: &str,
) -> Result<Option<Bound<'py, PyAny>>, TuiTestError> {
    dict.get_item(key)
        .map_err(|error| TuiTestError::usage(error.to_string()))
        .map(|value| value.filter(|value| !value.is_none()))
}

fn py_string(dict: &Bound<'_, PyDict>, key: &str) -> Result<Option<String>, TuiTestError> {
    py_item(dict, key)?
        .map(|value| {
            value
                .extract::<String>()
                .map_err(|error| TuiTestError::usage(format!("{key}: {error}")))
        })
        .transpose()
}

fn py_bool(dict: &Bound<'_, PyDict>, key: &str) -> Result<Option<bool>, TuiTestError> {
    py_item(dict, key)?
        .map(|value| {
            value
                .extract::<bool>()
                .map_err(|error| TuiTestError::usage(format!("{key}: {error}")))
        })
        .transpose()
}

fn py_usize(dict: &Bound<'_, PyDict>, key: &str) -> Result<Option<usize>, TuiTestError> {
    py_item(dict, key)?
        .map(|value| {
            integer_u64(&capture_integer(&value), key).and_then(|value| {
                usize::try_from(value)
                    .map_err(|_| TuiTestError::usage(format!("{key} is too large")))
            })
        })
        .transpose()
}

fn core_occurrence(
    value: Option<String>,
    nth: Option<usize>,
    name: &str,
) -> Result<MatchOccurrence, TuiTestError> {
    if let Some(index) = nth {
        if let Some(value) = value.as_deref().filter(|value| *value != "nth") {
            return Err(TuiTestError::usage(format!(
                "{name} cannot be used with occurrence '{value}'"
            )));
        }
        return Ok(MatchOccurrence::Nth(index));
    }
    match value.as_deref() {
        None | Some("any") => Ok(MatchOccurrence::Any),
        Some("unique") => Ok(MatchOccurrence::Unique),
        Some("first") => Ok(MatchOccurrence::First),
        Some("last") => Ok(MatchOccurrence::Last),
        Some("nth") => Err(TuiTestError::usage(format!("{name} requires an nth index"))),
        Some(value) => Err(TuiTestError::usage(format!(
            "{name} must be any, unique, first, last, or nth (got '{value}')"
        ))),
    }
}

fn core_style(dict: &Bound<'_, PyDict>) -> Result<TextStyle, TuiTestError> {
    Ok(TextStyle {
        foreground: py_string(dict, "foreground")?,
        background: py_string(dict, "background")?,
        bold: py_bool(dict, "bold")?,
        dim: py_bool(dict, "dim")?,
        italic: py_bool(dict, "italic")?,
        underline_style: py_string(dict, "underline_style")?,
        underline_color: py_string(dict, "underline_color")?,
        inverse: py_bool(dict, "inverse")?,
        hidden: py_bool(dict, "hidden")?,
        strikethrough: py_bool(dict, "strikethrough")?,
        blink: py_bool(dict, "blink")?,
    })
}

fn capture_locator_query(stages: &[Bound<'_, PyAny>]) -> Result<LocatorQuery, TuiTestError> {
    let mut parent = None;
    for (index, stage) in stages.iter().enumerate() {
        let dict = stage
            .cast::<PyDict>()
            .map_err(|error| TuiTestError::usage(format!("stages[{index}]: {error}")))?;
        let occurrence = core_occurrence(
            py_string(dict, "occurrence")?,
            py_usize(dict, "nth")?,
            &format!("stages[{index}].nth"),
        )?;
        let direction = match py_string(dict, "direction")?.as_deref() {
            None | Some("within") => LocatorDirection::Within,
            Some("after") => LocatorDirection::After,
            Some("before") => LocatorDirection::Before,
            Some(value) => {
                return Err(TuiTestError::usage(format!(
                    "locator direction must be within, after, or before (got '{value}')"
                )))
            }
        };
        let selector = match py_string(dict, "kind")?.as_deref() {
            Some("text") => {
                if py_item(dict, "style")?.is_some() {
                    return Err(TuiTestError::usage(
                        "text locator stages do not accept style parameters",
                    ));
                }
                let whitespace = match py_string(dict, "whitespace")?.as_deref() {
                    None | Some("exact") => WhitespaceMode::Exact,
                    Some("normalize") => WhitespaceMode::Normalize,
                    Some(value) => {
                        return Err(TuiTestError::usage(format!(
                            "whitespace must be exact or normalize (got '{value}')"
                        )))
                    }
                };
                LocatorSelector::Text(TextSelector {
                    text: py_string(dict, "text")?
                        .ok_or_else(|| TuiTestError::usage("text locator stage requires text"))?,
                    regex: py_bool(dict, "regex")?.unwrap_or(false),
                    full: py_bool(dict, "full")?.unwrap_or(false),
                    whitespace,
                    scope: Default::default(),
                })
            }
            Some("style") => {
                if py_item(dict, "text")?.is_some()
                    || py_bool(dict, "regex")?.unwrap_or(false)
                    || py_item(dict, "whitespace")?.is_some()
                {
                    return Err(TuiTestError::usage(
                        "style locator stages do not accept text parameters",
                    ));
                }
                let style = py_item(dict, "style")?
                    .ok_or_else(|| TuiTestError::usage("style locator stage requires style"))?;
                let style = style
                    .cast::<PyDict>()
                    .map_err(|error| TuiTestError::usage(error.to_string()))?;
                LocatorSelector::Style(StyleSelector {
                    style: core_style(style)?,
                    full: py_bool(dict, "full")?.unwrap_or(false),
                })
            }
            Some(value) => {
                return Err(TuiTestError::usage(format!(
                    "unknown locator stage kind '{value}'"
                )))
            }
            None => return Err(TuiTestError::usage("locator stage requires kind")),
        };
        parent = Some(LocatorQuery {
            selector,
            occurrence,
            within: parent.map(Box::new),
            direction,
            style: TextStyle::default(),
        });
    }
    parent.ok_or_else(|| TuiTestError::usage("locator requires at least one stage"))
}

fn profile_from_parts(
    scrollback: Option<&IntegerInput>,
    colors: &[(String, String)],
) -> Result<CoreProfile, TuiTestError> {
    let mut profile = CoreProfile::default();
    if let Some(scrollback) = scrollback {
        profile.scrollback =
            integer_unsigned(scrollback, "profile.scrollback", usize::MAX as u128)? as usize;
    }
    for (name, raw) in colors {
        let color = Rgb::parse(raw)
            .map_err(|error| TuiTestError::usage(format!("profile.colors.{name}: {error}")))?;
        if !profile.colors.set_named(name, color) {
            return Err(TuiTestError::usage(format!(
                "unknown profile color {name:?}"
            )));
        }
    }
    Ok(profile)
}

fn integer_unsigned(value: &IntegerInput, name: &str, maximum: u128) -> Result<u128, TuiTestError> {
    let parsed = match value {
        IntegerInput::NonNegative(value) => Some(*value as u128),
        IntegerInput::Negative(_) | IntegerInput::Invalid => None,
    };
    parsed.filter(|value| *value <= maximum).ok_or_else(|| {
        TuiTestError::usage(format!(
            "{name} must be an integer in the range 0..={maximum}"
        ))
    })
}

fn integer_i32(value: &IntegerInput, name: &str) -> Result<i32, TuiTestError> {
    let parsed = match value {
        IntegerInput::Negative(value) => Some(*value as i128),
        IntegerInput::NonNegative(value) => Some(*value as i128),
        IntegerInput::Invalid => None,
    };
    parsed
        .filter(|value| (i32::MIN as i128..=i32::MAX as i128).contains(value))
        .map(|value| value as i32)
        .ok_or_else(|| {
            TuiTestError::usage(format!(
                "{name} must be an integer in the range {}..={}",
                i32::MIN,
                i32::MAX
            ))
        })
}

fn parse_shell(value: Option<&str>) -> Result<Option<Shell>, TuiTestError> {
    value
        .map(|value| {
            let shell = match value {
                "bash" => Shell::Bash,
                "powershell" => Shell::Powershell,
                "pwsh" => Shell::Pwsh,
                "cmd" => Shell::Cmd,
                "fish" => Shell::Fish,
                "zsh" => Shell::Zsh,
                "xonsh" => Shell::Xonsh,
                "elvish" => Shell::Elvish,
                "nushell" => Shell::Nushell,
                other => {
                    return Err(TuiTestError::usage(format!(
                        "unknown shell '{other}'; expected bash, powershell, pwsh, cmd, fish, zsh, xonsh, elvish, or nushell"
                    )));
                }
            };
            Ok(shell)
        })
        .transpose()
}

fn parse_backend(value: Option<&str>) -> Result<Backend, TuiTestError> {
    value
        .unwrap_or("alacritty")
        .parse()
        .map_err(TuiTestError::usage)
}

fn parse_recording_format(value: Option<&str>) -> Result<Option<RecordingFormat>, TuiTestError> {
    value
        .map(|value| match value {
            "apng" => Ok(RecordingFormat::Apng),
            "gif" => Ok(RecordingFormat::Gif),
            "mp4" => Ok(RecordingFormat::Mp4),
            "cast" => Ok(RecordingFormat::Cast),
            other => Err(TuiTestError::usage(format!(
                "unknown recording format '{other}'; expected apng, gif, mp4, or cast"
            ))),
        })
        .transpose()
}

fn capture_background(
    background: Option<String>,
    transparent: bool,
) -> Result<Option<CaptureBackground>, TuiTestError> {
    if background.is_some() && transparent {
        return Err(TuiTestError::usage(
            "background and transparent options conflict",
        ));
    }
    if transparent {
        return Ok(Some(CaptureBackground::Transparent));
    }
    background
        .map(|value| CaptureBackground::parse(&value))
        .transpose()
}

fn unexpected_result(expected: &str) -> TuiTestError {
    TuiTestError::internal(format!(
        "native Python binding expected {expected}, but the engine returned another result type"
    ))
}

fn execute_unit(name: &str, operation: Operation) -> Result<(), TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Unit => Ok(()),
        _ => Err(unexpected_result("no value")),
    }
}

fn execute_open(name: &str, operation: Operation) -> Result<OpenResult, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Open(value) => Ok(value),
        _ => Err(unexpected_result("an open result")),
    }
}

fn execute_state(name: &str, operation: Operation) -> Result<State, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::State(value) => Ok(value),
        _ => Err(unexpected_result("terminal state")),
    }
}

fn execute_text(name: &str, operation: Operation) -> Result<String, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Text(value) => Ok(value),
        _ => Err(unexpected_result("terminal text")),
    }
}

fn execute_packed_screen(name: &str, operation: Operation) -> Result<PackedScreen, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::PackedScreen(value) => Ok(value),
        _ => Err(unexpected_result("a packed screen")),
    }
}

fn execute_cells(name: &str, operation: Operation) -> Result<Vec<Cell>, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Cells(value) => Ok(value),
        _ => Err(unexpected_result("terminal cells")),
    }
}

fn execute_matches(name: &str, operation: Operation) -> Result<Vec<TextMatch>, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Matches(value) => Ok(value),
        _ => Err(unexpected_result("text matches")),
    }
}

fn execute_command(name: &str, operation: Operation) -> Result<Option<String>, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Command(value) => Ok(value),
        _ => Err(unexpected_result("the last command")),
    }
}

fn execute_output(name: &str, operation: Operation) -> Result<Option<String>, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Output(value) => Ok(value),
        _ => Err(unexpected_result("the last output")),
    }
}

fn execute_exit_code(name: &str, operation: Operation) -> Result<Option<i32>, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::ExitCode(value) => Ok(value),
        _ => Err(unexpected_result("the last exit code")),
    }
}

fn execute_cwd(name: &str, operation: Operation) -> Result<Option<String>, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Cwd(value) => Ok(value),
        _ => Err(unexpected_result("the current working directory")),
    }
}

fn execute_title(name: &str, operation: Operation) -> Result<Option<String>, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Title(value) => Ok(value),
        _ => Err(unexpected_result("the window title")),
    }
}

fn execute_clipboard(name: &str, operation: Operation) -> Result<String, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Clipboard(value) => Ok(value),
        _ => Err(unexpected_result("the clipboard content")),
    }
}

fn execute_cursor(name: &str, operation: Operation) -> Result<Cursor, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Cursor(value) => Ok(value),
        _ => Err(unexpected_result("the cursor position")),
    }
}

fn execute_size(name: &str, operation: Operation) -> Result<Size, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Size(value) => Ok(value),
        _ => Err(unexpected_result("the terminal size")),
    }
}

fn execute_bell_count(name: &str, operation: Operation) -> Result<u64, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::BellCount(value) => Ok(value),
        _ => Err(unexpected_result("the terminal bell count")),
    }
}

fn execute_bell_events(name: &str, operation: Operation) -> Result<Vec<BellEvent>, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::BellEvents(value) => Ok(value),
        _ => Err(unexpected_result("the terminal bell events")),
    }
}

fn execute_snapshot(name: &str, operation: Operation) -> Result<SnapshotResult, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Snapshot(value) => Ok(value),
        _ => Err(unexpected_result("a snapshot status")),
    }
}

fn execute_screenshot(name: &str, operation: Operation) -> Result<ScreenshotResult, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Screenshot(value) => Ok(value),
        _ => Err(unexpected_result("a screenshot")),
    }
}

fn execute_recording(name: &str, operation: Operation) -> Result<String, TuiTestError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Recording(path) => Ok(path),
        _ => Err(unexpected_result("a recording path")),
    }
}

fn unit_to_py(py: Python<'_>, (): ()) -> PyResult<Py<PyAny>> {
    Ok(py.None())
}

fn string_to_py(py: Python<'_>, value: String) -> PyResult<Py<PyAny>> {
    Ok(value.into_pyobject(py)?.into_any().unbind())
}

fn optional_string_to_py(py: Python<'_>, value: Option<String>) -> PyResult<Py<PyAny>> {
    Ok(value.into_pyobject(py)?.into_any().unbind())
}

fn optional_i32_to_py(py: Python<'_>, value: Option<i32>) -> PyResult<Py<PyAny>> {
    Ok(value.into_pyobject(py)?.into_any().unbind())
}

fn u64_to_py(py: Python<'_>, value: u64) -> PyResult<Py<PyAny>> {
    Ok(value.into_pyobject(py)?.into_any().unbind())
}

fn string_list_to_py(py: Python<'_>, value: Vec<String>) -> PyResult<Py<PyAny>> {
    Ok(PyList::new(py, value)?.into_any().unbind())
}

fn open_to_py(py: Python<'_>, value: OpenResult) -> PyResult<Py<PyAny>> {
    let result = PyDict::new(py);
    result.set_item("shell_pid", value.shell_pid)?;
    result.set_item("session", value.session)?;
    result.set_item("ready", value.ready)?;
    result.set_item("recording", value.recording)?;
    Ok(result.into_any().unbind())
}

fn cursor_dict(py: Python<'_>, cursor: Cursor) -> PyResult<Bound<'_, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("x", cursor.x)?;
    value.set_item("y", cursor.y)?;
    Ok(value)
}

fn size_dict(py: Python<'_>, size: Size) -> PyResult<Bound<'_, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("cols", size.cols)?;
    value.set_item("rows", size.rows)?;
    Ok(value)
}

fn bell_events_to_py<'py>(py: Python<'py>, events: Vec<BellEvent>) -> PyResult<Bound<'py, PyList>> {
    let values = PyList::empty(py);
    for event in events {
        let value = PyDict::new(py);
        value.set_item("sequence", event.sequence)?;
        value.set_item("elapsed_ms", event.elapsed_ms)?;
        values.append(value)?;
    }
    Ok(values)
}

fn bell_events_to_py_object(py: Python<'_>, events: Vec<BellEvent>) -> PyResult<Py<PyAny>> {
    Ok(bell_events_to_py(py, events)?.into_any().unbind())
}

fn state_to_py(py: Python<'_>, value: State) -> PyResult<Py<PyAny>> {
    let result = PyDict::new(py);
    result.set_item("session_shell", value.session_shell)?;
    result.set_item("cols", value.cols)?;
    result.set_item("rows", value.rows)?;
    result.set_item("cursor", cursor_dict(py, value.cursor)?)?;
    result.set_item("title", value.title)?;
    result.set_item("cwd", value.cwd)?;
    result.set_item("last_command", value.last_command)?;
    result.set_item("last_exit", value.last_exit)?;
    result.set_item("exited", value.exited)?;
    result.set_item("ready", value.ready)?;
    result.set_item("bell_count", value.bell_count)?;
    let timeouts = PyDict::new(py);
    timeouts.set_item("text", value.timeouts.text)?;
    timeouts.set_item("idle", value.timeouts.idle)?;
    timeouts.set_item("command", value.timeouts.command)?;
    timeouts.set_item("exit", value.timeouts.exit)?;
    timeouts.set_item("ready", value.timeouts.ready)?;
    result.set_item("timeouts", timeouts)?;
    result.set_item("text", value.text)?;
    Ok(result.into_any().unbind())
}

fn set_color(value: &Bound<'_, PyDict>, key: &str, color: CellColor) -> PyResult<()> {
    match color {
        CellColor::Default => value.set_item(key, "default"),
        CellColor::Indexed(index) => value.set_item(key, index),
        CellColor::Rgb(red, green, blue) => {
            value.set_item(key, format!("#{red:02x}{green:02x}{blue:02x}"))
        }
    }
}

fn cell_to_py(py: Python<'_>, cell: Cell) -> PyResult<Bound<'_, PyDict>> {
    let value = PyDict::new(py);
    value.set_item("x", cell.x)?;
    value.set_item("y", cell.y)?;
    value.set_item("char", cell.char)?;
    set_color(&value, "fg", cell.fg)?;
    set_color(&value, "bg", cell.bg)?;
    value.set_item("bold", cell.bold)?;
    value.set_item("dim", cell.dim)?;
    value.set_item("italic", cell.italic)?;
    value.set_item("inverse", cell.inverse)?;
    value.set_item("invisible", cell.invisible)?;
    value.set_item("strike", cell.strike)?;
    value.set_item("blink", cell.blink)?;
    value.set_item("underline", cell.underline)?;
    value.set_item("underline_style", cell.underline_style)?;
    set_color(&value, "underline_color", cell.underline_color)?;
    Ok(value)
}

fn cells_to_py(py: Python<'_>, cells: Vec<Cell>) -> PyResult<Py<PyAny>> {
    let values = PyList::empty(py);
    for cell in cells {
        values.append(cell_to_py(py, cell)?)?;
    }
    Ok(values.into_any().unbind())
}

fn matches_to_py(py: Python<'_>, matches: Vec<TextMatch>) -> PyResult<Py<PyAny>> {
    let values = PyList::empty(py);
    for matched in matches {
        let value = PyDict::new(py);
        value.set_item("text", matched.text)?;
        let start = PyDict::new(py);
        start.set_item("row", matched.start.row)?;
        start.set_item("column", matched.start.column)?;
        value.set_item("start", start)?;
        let end = PyDict::new(py);
        end.set_item("row", matched.end.row)?;
        end.set_item("column", matched.end.column)?;
        value.set_item("end", end)?;
        let spans = PyList::empty(py);
        for span in matched.spans {
            let item = PyDict::new(py);
            item.set_item("row", span.row)?;
            item.set_item("start", span.start)?;
            item.set_item("end", span.end)?;
            spans.append(item)?;
        }
        value.set_item("spans", spans)?;
        values.append(value)?;
    }
    Ok(values.into_any().unbind())
}

fn cursor_to_py(py: Python<'_>, cursor: Cursor) -> PyResult<Py<PyAny>> {
    Ok(cursor_dict(py, cursor)?.into_any().unbind())
}

fn size_to_py(py: Python<'_>, size: Size) -> PyResult<Py<PyAny>> {
    Ok(size_dict(py, size)?.into_any().unbind())
}

fn snapshot_to_py(py: Python<'_>, status: SnapshotResult) -> PyResult<Py<PyAny>> {
    let value = match status {
        SnapshotResult::Passed => "passed",
        SnapshotResult::Written => "written",
        SnapshotResult::Updated => "updated",
    };
    string_to_py(py, value.to_string())
}

fn screenshot_to_py(py: Python<'_>, screenshot: ScreenshotResult) -> PyResult<Py<PyAny>> {
    match screenshot {
        ScreenshotResult::Path(path) | ScreenshotResult::Text(path) => string_to_py(py, path),
    }
}

fn packed_screen_to_py(py: Python<'_>, screen: PackedScreen) -> PyResult<Py<PyAny>> {
    let bytes = PyBytes::new(py, &screen.utf8);
    let view = PyMemoryView::from(bytes.as_any())?;
    let values = [
        view.into_any().unbind(),
        screen.cols.into_pyobject(py)?.into_any().unbind(),
        screen.rows.into_pyobject(py)?.into_any().unbind(),
    ];
    Ok(PyTuple::new(py, values)?.into_any().unbind())
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NativeSession>()?;
    m.add(
        "NativeAssertionError",
        m.py().get_type::<NativeAssertionError>(),
    )?;
    m.add("NativeUsageError", m.py().get_type::<NativeUsageError>())?;
    m.add(
        "NativeNoSessionError",
        m.py().get_type::<NativeNoSessionError>(),
    )?;
    m.add(
        "NativeInternalError",
        m.py().get_type::<NativeInternalError>(),
    )?;
    m.add_function(wrap_pyfunction!(sessions, m)?)?;
    m.add_function(wrap_pyfunction!(close_all, m)?)?;
    m.add_function(wrap_pyfunction!(recording, m)?)?;
    m.add_function(wrap_pyfunction!(panic_probe, m)?)?;
    m.add_function(wrap_pyfunction!(_close_all_blocking, m)?)?;
    Ok(())
}
