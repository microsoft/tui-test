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
    operation_name: &'static str,
    operation: impl FnOnce() -> Result<Operation>,
) -> *mut TuiResult {
    boundary(|| {
        let (session, context) = unsafe { input::session(session)? };
        global_registry()
            .session(session)
            .execute_with_context(operation()?, context.with_operation(operation_name))
    })
}

fn finish_trace_and_close(
    finish_trace: impl FnOnce() -> Result<OperationResult>,
    close: impl FnOnce() -> Result<OperationResult>,
) -> Result<OperationResult> {
    let trace = finish_trace();
    let closed = close();
    if let Err(error) = trace {
        if error.kind != ErrorKind::NoSession {
            return Err(error);
        }
    }
    closed
}
#[no_mangle]
pub extern "C" fn tui_abi_version() -> u32 {
    4
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
    unsafe {
        execute(session, "open", || {
            Ok(Operation::Open(input::open(options)?))
        })
    }
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
        execute(session, "run", || {
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
pub unsafe extern "C" fn tui_close(session: TuiString, failed: TuiOptionalBool) -> *mut TuiResult {
    boundary(|| {
        let (name, context) = unsafe { input::session(session)? };
        let handle = global_registry().session(name);
        if let Some(failed) = failed.option() {
            return finish_trace_and_close(
                || {
                    handle.execute_with_context(
                        Operation::FinishTrace { failed },
                        context.clone().with_operation("trace.finish"),
                    )
                },
                || {
                    handle.execute_with_context(
                        Operation::Close,
                        context.clone().with_operation("close"),
                    )
                },
            );
        }
        handle.execute_with_context(Operation::Close, context.with_operation("close"))
    })
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths.
pub unsafe extern "C" fn tui_restart(
    session: TuiString,
    graceful_timeout_ms: u64,
) -> *mut TuiResult {
    unsafe {
        execute(session, "restart", || {
            Ok(Operation::Restart {
                graceful_timeout_ms,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_state(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "state", || Ok(Operation::State)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_command(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getCommand", || Ok(Operation::GetCommand)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_output(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getOutput", || Ok(Operation::GetOutput)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_exit_code(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getExitCode", || Ok(Operation::GetExitCode)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_cwd(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getCwd", || Ok(Operation::GetCwd)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_cursor(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getCursor", || Ok(Operation::GetCursor)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_size(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getSize", || Ok(Operation::GetSize)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_title(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getTitle", || Ok(Operation::GetTitle)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_clipboard(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getClipboard", || Ok(Operation::GetClipboard)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_bell_count(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getBellCount", || Ok(Operation::GetBellCount)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_get_bell_events(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "getBellEvents", || Ok(Operation::GetBellEvents)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_stop_recording(session: TuiString) -> *mut TuiResult {
    unsafe { execute(session, "stopRecording", || Ok(Operation::StopRecording)) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_text(session: TuiString, full: bool) -> *mut TuiResult {
    unsafe { execute(session, "text", || Ok(Operation::Text { full })) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_packed_screen(session: TuiString, full: bool) -> *mut TuiResult {
    unsafe {
        execute(session, "packedScreen", || {
            Ok(Operation::PackedScreen { full })
        })
    }
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
    unsafe { execute(session, "cells", || Ok(Operation::Cells { x, y, w, h })) }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_write(session: TuiString, text: TuiString) -> *mut TuiResult {
    unsafe {
        execute(session, "write", || {
            Ok(Operation::Write {
                data: text.required()?,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths.
pub unsafe extern "C" fn tui_type(session: TuiString, text: TuiString) -> *mut TuiResult {
    unsafe {
        execute(session, "type", || {
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
        execute(session, "submit", || {
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
        execute(session, "signal", || {
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
    let (operation_name, key_action) = match action {
        0 => ("press", KeyAction::Press),
        1 => ("keyDown", KeyAction::Down),
        2 => ("repeat", KeyAction::Repeat),
        3 => ("keyUp", KeyAction::Up),
        _ => return boundary(|| Err(TuiTestError::usage("unknown key action"))),
    };
    unsafe {
        execute(session, operation_name, || {
            Ok(Operation::Key {
                keys: input::strings(keys, len)?,
                action: key_action,
            })
        })
    }
}
#[no_mangle]
/// # Safety
/// Borrowed input buffers must be valid and readable for their stated lengths
/// throughout this call; see TuiString and the input structure contracts.
pub unsafe extern "C" fn tui_resize(session: TuiString, cols: u16, rows: u16) -> *mut TuiResult {
    unsafe { execute(session, "resize", || Ok(Operation::Resize { cols, rows })) }
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
        execute(session, "mouse.click", || {
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
        execute(session, "mouse.move", || {
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
        execute(session, "mouse.down", || {
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
        execute(session, "mouse.up", || {
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
        execute(session, "mouse.drag", || {
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
        execute(session, "mouse.scroll", || {
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
        execute(session, "waitTitle", || {
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
        execute(session, "expectTitle", || {
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
        execute(session, "waitClipboard", || {
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
        execute(session, "waitIdle", || {
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
        execute(session, "waitCommand", || {
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
        execute(session, "waitExit", || {
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
        execute(session, "waitReady", || {
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
        execute(session, "waitBell", || {
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
pub unsafe extern "C" fn tui_find_locator(
    session: TuiString,
    query: TuiQuery,
    require_one: bool,
) -> *mut TuiResult {
    let operation_name = if require_one {
        "locator.location"
    } else {
        "locator.locations"
    };
    unsafe {
        execute(session, operation_name, || {
            let query = input::query(query)?;
            Ok(if require_one {
                Operation::ResolveLocator { query }
            } else {
                Operation::FindLocator { query }
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
        execute(session, "locator.wait", || {
            Ok(Operation::WaitLocator {
                query: input::query(query)?,
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
pub unsafe extern "C" fn tui_expect_locator(
    session: TuiString,
    query: TuiQuery,
    options: TuiWaitOptions,
) -> *mut TuiResult {
    unsafe {
        execute(session, "locator.expect", || {
            Ok(Operation::WaitLocator {
                query: input::query(query)?,
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
        execute(session, "locator.click", || {
            Ok(Operation::ClickLocator {
                query: input::query(query)?,
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
        execute(session, "locator.highlight", || {
            Ok(Operation::HighlightLocator {
                query: input::query(query)?,
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
        execute(session, "expectExitCode", || {
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
        execute(session, "expectOutput", || {
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
        execute(session, "expectBellCount", || {
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
    include_style: bool,
    include_title: bool,
    cwd: TuiString,
) -> *mut TuiResult {
    unsafe {
        execute(session, "expectSnapshot", || {
            Ok(Operation::Snapshot {
                name: name.required()?,
                update,
                include_style,
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
    options: *const TuiScreenshotOptions,
) -> *mut TuiResult {
    if options.is_null() {
        return boundary(|| Err(TuiTestError::usage("screenshot options pointer is null")));
    }
    let options = unsafe { *options };
    unsafe {
        execute(session, "screenshot", || {
            Ok(Operation::Screenshot {
                full: options.full,
                path: options.path.optional()?,
                zoom: options.zoom.option(),
                background: input::capture_background(options.background, options.transparent)?,
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
    unsafe { execute(session, "startRecording", || input::recording(options)) }
}
#[cfg(test)]
mod tests;
