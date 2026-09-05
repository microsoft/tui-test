//! Private typed C ABI for the Go binding. All borrowed input buffers must
//! remain valid for the call. Inputs are copied before entering the engine;
//! output storage belongs to Rust until tui_result_free is called.
mod input;
mod output;
mod types;
use std::panic::{catch_unwind, AssertUnwindSafe};
use tui_test::api::*;
use tui_test::runtime::global_registry;
pub use types::*;

type Result<T> = std::result::Result<T, TuiTestError>;
fn boundary(f: impl FnOnce() -> Result<OperationResult>) -> *mut TuiResult {
    match catch_unwind(AssertUnwindSafe(|| f().map(output::operation))) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => output::error(error),
        Err(_) => output::error(TuiTestError::internal("native Go adapter panicked")),
    }
}
unsafe fn execute(
    session: TuiString,
    operation: impl FnOnce() -> Result<Operation>,
) -> *mut TuiResult {
    boundary(|| {
        let session = unsafe { session.required()? };
        global_registry().session(session).execute(operation()?)
    })
}
#[no_mangle]
pub extern "C" fn tui_abi_version() -> u32 {
    1
}
#[no_mangle]
/// # Safety
/// result must be NULL or a live result returned by this library, freed once.
pub unsafe extern "C" fn tui_result_free(result: *mut TuiResult) {
    unsafe { output::free(result) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_open(session: TuiString, options: TuiOpenOptions) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::Open(input::open(options)?))) }
}
#[no_mangle]
/// Pointer form for foreign callers with limited by-value argument space.
///
/// # Safety
/// options must be NULL or point to a valid TuiOpenOptions for this call.
/// Its borrowed buffers follow the same contract as tui_open.
pub unsafe extern "C" fn tui_open_ptr(
    session: TuiString,
    options: *const TuiOpenOptions,
) -> *mut TuiResult {
    if options.is_null() {
        return boundary(|| Err(TuiTestError::usage("open options pointer is null")));
    }
    unsafe { tui_open(session, *options) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_run(
    session: TuiString,
    options: TuiOpenOptions,
    program: TuiString,
    args: *const TuiString,
    args_len: usize,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            let o = input::open(options)?;
            let program = program.required()?;
            if program.is_empty() {
                return Err(TuiTestError::usage("program must not be empty"));
            }
            Ok(Operation::Run(RunOptions {
                program,
                args: input::strings(args, args_len)?,
                backend: o.backend,
                profile: o.profile,
                cols: o.cols,
                rows: o.rows,
                cwd: o.cwd,
                env: o.env,
                wait_ready: o.wait_ready,
                restart: o.restart,
                timeouts: o.timeouts,
                recording: o.recording,
            }))
        })
    }
}
#[no_mangle]
/// Pointer form for foreign callers with limited by-value argument space.
///
/// # Safety
/// options must be NULL or point to a valid TuiOpenOptions for this call.
/// All borrowed buffers follow the same contract as tui_run.
pub unsafe extern "C" fn tui_run_ptr(
    session: TuiString,
    options: *const TuiOpenOptions,
    program: TuiString,
    args: *const TuiString,
    args_len: usize,
) -> *mut TuiResult {
    if options.is_null() {
        return boundary(|| Err(TuiTestError::usage("run options pointer is null")));
    }
    unsafe { tui_run(session, *options, program, args, args_len) }
}
#[no_mangle]
pub extern "C" fn tui_sessions() -> *mut TuiResult {
    match catch_unwind(AssertUnwindSafe(|| {
        output::sessions(global_registry().sessions())
    })) {
        Ok(value) => value,
        Err(_) => output::error(TuiTestError::internal("native Go adapter panicked")),
    }
}
#[no_mangle]
pub extern "C" fn tui_close_all() -> *mut TuiResult {
    boundary(|| {
        global_registry().close_all();
        Ok(OperationResult::Unit)
    })
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_recording(session: TuiString) -> *mut TuiResult {
    boundary(|| {
        let session = unsafe { session.required()? };
        global_registry()
            .recording(&session)
            .map(OperationResult::Recording)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    TuiTestError::new(
                        ErrorKind::NoSession,
                        format!("no recording for session '{session}'"),
                    )
                } else {
                    TuiTestError::internal(format!(
                        "failed to read recording for session '{session}': {e}"
                    ))
                }
            })
    })
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_close(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::Close)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_state(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::State)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_command(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetCommand)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_output(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetOutput)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_exit_code(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetExitCode)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_cwd(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetCwd)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_cursor(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetCursor)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_size(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetSize)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_title(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetTitle)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_clipboard(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetClipboard)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_bell_count(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetBellCount)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_bell_events(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::GetBellEvents)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_stop_recording(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::StopRecording)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_text(session: TuiString, full: bool) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::Text { full })) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_packed_screen(session: TuiString, full: bool) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::PackedScreen { full })) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_cells(
    session: TuiString,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::Cells { x, y, w, h })) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_write(session: TuiString, text: TuiString) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Write {
                data: text.required()?,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_submit(session: TuiString, text: TuiString) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Submit {
                data: text.optional()?,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_signal(session: TuiString, text: TuiString) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Signal {
                name: text.required()?,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_key(
    session: TuiString,
    keys: *const TuiString,
    len: usize,
    action: u32,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Key {
                keys: input::strings(keys, len)?,
                action: match action {
                    0 => KeyAction::Press,
                    1 => KeyAction::Down,
                    2 => KeyAction::Repeat,
                    3 => KeyAction::Up,
                    _ => return Err(TuiTestError::usage("unknown key action")),
                },
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_resize(session: TuiString, cols: u16, rows: u16) -> *mut TuiResult {
    unsafe { execute(session, || Ok(Operation::Resize { cols, rows })) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_mouse_click(
    session: TuiString,
    x: TuiOptionalU64,
    y: TuiOptionalU64,
    on_text: TuiString,
    options: TuiMouseOptions,
    clicks: u8,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Mouse {
                action: MouseAction::Click {
                    x: input::u16_option(x, "x")?,
                    y: input::u16_option(y, "y")?,
                    on_text: on_text.optional()?,
                    options: input::mouse(options)?,
                    clicks,
                },
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_mouse_move(session: TuiString, x: u16, y: u16) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Mouse {
                action: MouseAction::Move { x, y },
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_mouse_down(
    session: TuiString,
    x: u16,
    y: u16,
    options: TuiMouseOptions,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Mouse {
                action: MouseAction::Down {
                    x,
                    y,
                    options: input::mouse(options)?,
                },
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_mouse_up(
    session: TuiString,
    x: u16,
    y: u16,
    options: TuiMouseOptions,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Mouse {
                action: MouseAction::Up {
                    x,
                    y,
                    options: input::mouse(options)?,
                },
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_mouse_drag(
    session: TuiString,
    x1: u16,
    y1: u16,
    x2: u16,
    y2: u16,
    options: TuiMouseOptions,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Mouse {
                action: MouseAction::Drag {
                    x1,
                    y1,
                    x2,
                    y2,
                    options: input::mouse(options)?,
                },
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_mouse_scroll(
    session: TuiString,
    direction: TuiString,
    amount: u16,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Mouse {
                action: MouseAction::Scroll {
                    direction: direction.required()?,
                    amount,
                },
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_wait_title(
    session: TuiString,
    text: TuiString,
    options: TuiWaitOptions,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::WaitTitle {
                text: text.required()?,
                regex: options.regex,
                not: options.not,
                timeout_ms: options.timeout_ms.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_expect_title(
    session: TuiString,
    text: TuiString,
    options: TuiWaitOptions,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::ExpectTitle {
                text: text.required()?,
                regex: options.regex,
                not: options.not,
                timeout_ms: options.timeout_ms.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_wait_clipboard(
    session: TuiString,
    text: TuiString,
    options: TuiWaitOptions,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            let timeout_ms = options.timeout_ms.option();
            match text.optional()? {
                Some(text) => Ok(Operation::WaitClipboardMatch {
                    pattern: if options.regex {
                        ClipboardPattern::regex(&text)
                            .map_err(|e| TuiTestError::usage(format!("invalid regex: {e}")))?
                    } else {
                        text.into()
                    },
                    timeout_ms,
                }),
                None if options.regex => Err(TuiTestError::usage("clipboard regex requires text")),
                None => Ok(Operation::WaitClipboard { timeout_ms }),
            }
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_wait_idle(
    session: TuiString,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::WaitIdle {
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_wait_command(
    session: TuiString,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::WaitCommand {
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_wait_exit(
    session: TuiString,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::WaitExit {
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_wait_ready(
    session: TuiString,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::WaitReady {
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_wait_bell(
    session: TuiString,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::WaitBell {
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_find_locator(session: TuiString, query: TuiQuery) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::FindLocator {
                query: input::query(query, false)?,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_wait_locator(
    session: TuiString,
    query: TuiQuery,
    options: TuiWaitOptions,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::WaitLocator {
                query: input::query(query, false)?,
                not: options.not,
                timeout_ms: options.timeout_ms.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_click_locator(
    session: TuiString,
    query: TuiQuery,
    options: TuiMouseOptions,
    clicks: u8,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::ClickLocator {
                query: input::query(query, true)?,
                options: input::mouse(options)?,
                clicks,
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_highlight_locator(
    session: TuiString,
    query: TuiQuery,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::HighlightLocator {
                query: input::query(query, false)?,
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_expect_exit_code(
    session: TuiString,
    code: i32,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::ExpectExitCode {
                code,
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_expect_output(
    session: TuiString,
    text: TuiString,
    regex: bool,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::ExpectOutput {
                text: text.required()?,
                regex,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_expect_bell_count(
    session: TuiString,
    count: u64,
    timeout: TuiOptionalU64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::ExpectBellCount {
                count,
                timeout_ms: timeout.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_snapshot(
    session: TuiString,
    name: TuiString,
    update: bool,
    include_colors: bool,
    include_title: bool,
    cwd: TuiString,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Snapshot {
                name: name.required()?,
                update,
                include_colors,
                include_title,
                cwd: cwd.optional()?,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_screenshot(
    session: TuiString,
    full: bool,
    path: TuiString,
    zoom: TuiOptionalF64,
) -> *mut TuiResult {
    unsafe {
        execute(session, || {
            Ok(Operation::Screenshot {
                full,
                path: path.optional()?,
                zoom: zoom.option(),
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_start_recording(
    session: TuiString,
    options: TuiRecordingOptions,
) -> *mut TuiResult {
    unsafe { execute(session, || input::recording(options)) }
}
#[cfg(test)]
mod tests;
