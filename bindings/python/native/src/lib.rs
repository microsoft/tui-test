use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyInt, PyList, PyMemoryView, PyModule, PyTuple};
use shell_use::runtime::global_registry;
use shell_use::shell::Shell;
use shell_use::{
    Cell, CellColor, Cursor, ErrorKind, MouseAction, OpenOptions, OpenResult, Operation,
    OperationResult, PackedScreen, RunOptions, ScreenshotResult, ShellUseError, Size,
    SnapshotResult, State, Timeouts,
};

pyo3::create_exception!(
    shell_use._native,
    NativeAssertionError,
    PyException,
    "Native assertion failure."
);
pyo3::create_exception!(
    shell_use._native,
    NativeUsageError,
    PyException,
    "Native usage error."
);
pyo3::create_exception!(
    shell_use._native,
    NativeNoSessionError,
    PyException,
    "Native session was not found."
);
pyo3::create_exception!(
    shell_use._native,
    NativeInternalError,
    PyException,
    "Native internal error."
);

#[pyclass(module = "shell_use._native", frozen)]
struct NativeSession {
    #[pyo3(get)]
    name: String,
}

#[pymethods]
impl NativeSession {
    #[new]
    fn new(name: String) -> Self {
        Self { name }
    }

    #[pyo3(signature = (
        shell,
        cols,
        rows,
        cwd,
        env,
        wait_ready,
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
        cols: Bound<'py, PyAny>,
        rows: Bound<'py, PyAny>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        wait_ready: Option<bool>,
        text_timeout: Option<Bound<'py, PyAny>>,
        idle_timeout: Option<Bound<'py, PyAny>>,
        command_timeout: Option<Bound<'py, PyAny>>,
        exit_timeout: Option<Bound<'py, PyAny>>,
        ready_timeout: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let cols = capture_integer(&cols);
        let rows = capture_integer(&rows);
        let text_timeout = capture_optional_integer(text_timeout);
        let idle_timeout = capture_optional_integer(idle_timeout);
        let command_timeout = capture_optional_integer(command_timeout);
        let exit_timeout = capture_optional_integer(exit_timeout);
        let ready_timeout = capture_optional_integer(ready_timeout);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_open(
                    &name,
                    Operation::Open(OpenOptions {
                        profile: Default::default(),
                        shell: parse_shell(shell.as_deref())?,
                        cols: integer_u16(&cols, "cols")?,
                        rows: integer_u16(&rows, "rows")?,
                        cwd,
                        env,
                        wait_ready,
                        timeouts: Timeouts {
                            text: optional_u64(text_timeout.as_ref(), "text_timeout")?,
                            idle: optional_u64(idle_timeout.as_ref(), "idle_timeout")?,
                            command: optional_u64(command_timeout.as_ref(), "command_timeout")?,
                            exit: optional_u64(exit_timeout.as_ref(), "exit_timeout")?,
                            ready: optional_u64(ready_timeout.as_ref(), "ready_timeout")?,
                        },
                    }),
                )
            },
            open_to_py,
        )
    }

    #[pyo3(signature = (
        program,
        args,
        cols,
        rows,
        cwd,
        env,
        wait_ready,
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
        cols: Bound<'py, PyAny>,
        rows: Bound<'py, PyAny>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
        wait_ready: Option<bool>,
        text_timeout: Option<Bound<'py, PyAny>>,
        idle_timeout: Option<Bound<'py, PyAny>>,
        command_timeout: Option<Bound<'py, PyAny>>,
        exit_timeout: Option<Bound<'py, PyAny>>,
        ready_timeout: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let cols = capture_integer(&cols);
        let rows = capture_integer(&rows);
        let text_timeout = capture_optional_integer(text_timeout);
        let idle_timeout = capture_optional_integer(idle_timeout);
        let command_timeout = capture_optional_integer(command_timeout);
        let exit_timeout = capture_optional_integer(exit_timeout);
        let ready_timeout = capture_optional_integer(ready_timeout);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_open(
                    &name,
                    Operation::Run(RunOptions {
                        profile: Default::default(),
                        program,
                        args,
                        cols: integer_u16(&cols, "cols")?,
                        rows: integer_u16(&rows, "rows")?,
                        cwd,
                        env,
                        wait_ready,
                        timeouts: Timeouts {
                            text: optional_u64(text_timeout.as_ref(), "text_timeout")?,
                            idle: optional_u64(idle_timeout.as_ref(), "idle_timeout")?,
                            command: optional_u64(command_timeout.as_ref(), "command_timeout")?,
                            exit: optional_u64(exit_timeout.as_ref(), "exit_timeout")?,
                            ready: optional_u64(ready_timeout.as_ref(), "ready_timeout")?,
                        },
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
            move || execute_unit(&name, Operation::Press { keys }),
            unit_to_py,
        )
    }

    fn keys<'py>(&self, py: Python<'py>, combo: String) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_unit(&name, Operation::Press { keys: vec![combo] }),
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
                            button: integer_u8(&button, "button")?,
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
                            button: integer_u8(&button, "button")?,
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
                            button: integer_u8(&button, "button")?,
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
                            button: integer_u8(&button, "button")?,
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

    #[pyo3(signature = (text, regex, full, not_, timeout_ms))]
    fn wait_text<'py>(
        &self,
        py: Python<'py>,
        text: String,
        regex: bool,
        full: bool,
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
                    Operation::WaitText {
                        text,
                        regex,
                        full,
                        timeout_ms: optional_u64(timeout_ms.as_ref(), "timeout")?,
                        not: not_,
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

    #[pyo3(signature = (text, regex, full, strict, not_, fg, bg, timeout_ms))]
    #[allow(clippy::too_many_arguments)]
    fn expect_text<'py>(
        &self,
        py: Python<'py>,
        text: String,
        regex: bool,
        full: bool,
        strict: bool,
        not_: bool,
        fg: Option<String>,
        bg: Option<String>,
        timeout_ms: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_ms = capture_optional_integer(timeout_ms);
        let name = self.name.clone();
        future_blocking(
            py,
            move || {
                execute_unit(
                    &name,
                    Operation::ExpectText {
                        text,
                        regex,
                        full,
                        strict,
                        not: not_,
                        fg,
                        bg,
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

    #[pyo3(signature = (path, full))]
    fn screenshot<'py>(
        &self,
        py: Python<'py>,
        path: Option<String>,
        full: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let name = self.name.clone();
        future_blocking(
            py,
            move || execute_screenshot(&name, Operation::Screenshot { full, path }),
            screenshot_to_py,
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
        || -> Result<(), ShellUseError> {
            panic!("shell-use Python panic probe");
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
    F: FnOnce() -> Result<T, ShellUseError> + Send + 'static,
{
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let value = run_blocking(task).await.map_err(shell_error_to_py)?;
        Python::attach(|py| convert(py, value))
    })
}

async fn run_blocking<T, F>(task: F) -> Result<T, ShellUseError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ShellUseError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        catch_unwind(AssertUnwindSafe(task)).unwrap_or_else(|payload| {
            Err(ShellUseError::internal(format!(
                "native Python operation panicked: {}",
                panic_message(payload.as_ref())
            )))
        })
    })
    .await
    .map_err(|error| ShellUseError::internal(format!("native Python worker failed: {error}")))?
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

fn shell_error_to_py(error: ShellUseError) -> PyErr {
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

fn io_error_to_shell_error(error: std::io::Error) -> ShellUseError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ShellUseError::new(ErrorKind::NoSession, error.to_string())
    } else {
        ShellUseError::internal(error.to_string())
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

fn integer_u8(value: &IntegerInput, name: &str) -> Result<u8, ShellUseError> {
    integer_unsigned(value, name, u8::MAX as u128).map(|value| value as u8)
}

fn integer_u16(value: &IntegerInput, name: &str) -> Result<u16, ShellUseError> {
    integer_unsigned(value, name, u16::MAX as u128).map(|value| value as u16)
}

fn integer_u64(value: &IntegerInput, name: &str) -> Result<u64, ShellUseError> {
    integer_unsigned(value, name, u64::MAX as u128).map(|value| value as u64)
}

fn optional_u64(value: Option<&IntegerInput>, name: &str) -> Result<Option<u64>, ShellUseError> {
    value.map(|value| integer_u64(value, name)).transpose()
}

fn integer_unsigned(
    value: &IntegerInput,
    name: &str,
    maximum: u128,
) -> Result<u128, ShellUseError> {
    let parsed = match value {
        IntegerInput::NonNegative(value) => Some(*value as u128),
        IntegerInput::Negative(_) | IntegerInput::Invalid => None,
    };
    parsed.filter(|value| *value <= maximum).ok_or_else(|| {
        ShellUseError::usage(format!(
            "{name} must be an integer in the range 0..={maximum}"
        ))
    })
}

fn integer_i32(value: &IntegerInput, name: &str) -> Result<i32, ShellUseError> {
    let parsed = match value {
        IntegerInput::Negative(value) => Some(*value as i128),
        IntegerInput::NonNegative(value) => Some(*value as i128),
        IntegerInput::Invalid => None,
    };
    parsed
        .filter(|value| (i32::MIN as i128..=i32::MAX as i128).contains(value))
        .map(|value| value as i32)
        .ok_or_else(|| {
            ShellUseError::usage(format!(
                "{name} must be an integer in the range {}..={}",
                i32::MIN,
                i32::MAX
            ))
        })
}

fn parse_shell(value: Option<&str>) -> Result<Option<Shell>, ShellUseError> {
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
                    return Err(ShellUseError::usage(format!(
                        "unknown shell '{other}'; expected bash, powershell, pwsh, cmd, fish, zsh, xonsh, elvish, or nushell"
                    )));
                }
            };
            Ok(shell)
        })
        .transpose()
}

fn unexpected_result(expected: &str) -> ShellUseError {
    ShellUseError::internal(format!(
        "native Python binding expected {expected}, but the engine returned another result type"
    ))
}

fn execute_unit(name: &str, operation: Operation) -> Result<(), ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Unit => Ok(()),
        _ => Err(unexpected_result("no value")),
    }
}

fn execute_open(name: &str, operation: Operation) -> Result<OpenResult, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Open(value) => Ok(value),
        _ => Err(unexpected_result("an open result")),
    }
}

fn execute_state(name: &str, operation: Operation) -> Result<State, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::State(value) => Ok(value),
        _ => Err(unexpected_result("terminal state")),
    }
}

fn execute_text(name: &str, operation: Operation) -> Result<String, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Text(value) => Ok(value),
        _ => Err(unexpected_result("terminal text")),
    }
}

fn execute_packed_screen(name: &str, operation: Operation) -> Result<PackedScreen, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::PackedScreen(value) => Ok(value),
        _ => Err(unexpected_result("a packed screen")),
    }
}

fn execute_cells(name: &str, operation: Operation) -> Result<Vec<Cell>, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Cells(value) => Ok(value),
        _ => Err(unexpected_result("terminal cells")),
    }
}

fn execute_command(name: &str, operation: Operation) -> Result<Option<String>, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Command(value) => Ok(value),
        _ => Err(unexpected_result("the last command")),
    }
}

fn execute_output(name: &str, operation: Operation) -> Result<Option<String>, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Output(value) => Ok(value),
        _ => Err(unexpected_result("the last output")),
    }
}

fn execute_exit_code(name: &str, operation: Operation) -> Result<Option<i32>, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::ExitCode(value) => Ok(value),
        _ => Err(unexpected_result("the last exit code")),
    }
}

fn execute_cwd(name: &str, operation: Operation) -> Result<Option<String>, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Cwd(value) => Ok(value),
        _ => Err(unexpected_result("the current working directory")),
    }
}

fn execute_title(name: &str, operation: Operation) -> Result<Option<String>, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Title(value) => Ok(value),
        _ => Err(unexpected_result("the window title")),
    }
}

fn execute_cursor(name: &str, operation: Operation) -> Result<Cursor, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Cursor(value) => Ok(value),
        _ => Err(unexpected_result("the cursor position")),
    }
}

fn execute_size(name: &str, operation: Operation) -> Result<Size, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Size(value) => Ok(value),
        _ => Err(unexpected_result("the terminal size")),
    }
}

fn execute_snapshot(name: &str, operation: Operation) -> Result<SnapshotResult, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Snapshot(value) => Ok(value),
        _ => Err(unexpected_result("a snapshot status")),
    }
}

fn execute_screenshot(name: &str, operation: Operation) -> Result<ScreenshotResult, ShellUseError> {
    match global_registry().execute(name, operation)? {
        OperationResult::Screenshot(value) => Ok(value),
        _ => Err(unexpected_result("a screenshot")),
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
