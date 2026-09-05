use crate::types::*;
use tui_test::api::*;

#[derive(Default)]
struct Storage {
    bytes: Vec<Box<[u8]>>,
    cells: Vec<TuiCell>,
    matches: Vec<TuiMatch>,
    spans: Vec<Box<[TuiSpan]>>,
    bells: Vec<TuiBellEvent>,
    strings: Vec<TuiString>,
}
impl Storage {
    fn bytes(&mut self, value: Vec<u8>) -> TuiString {
        let value = value.into_boxed_slice();
        let output = TuiString {
            data: value.as_ptr(),
            len: value.len(),
        };
        self.bytes.push(value);
        output
    }
    fn string(&mut self, value: String) -> TuiString {
        self.bytes(value.into_bytes())
    }
    fn optional(&mut self, value: Option<String>) -> TuiString {
        value.map(|s| self.string(s)).unwrap_or_default()
    }
}
fn integer(value: Option<i32>) -> TuiOptionalI32 {
    TuiOptionalI32 {
        present: value.is_some(),
        value: value.unwrap_or_default(),
    }
}
fn unsigned(value: u64) -> TuiOptionalU64 {
    TuiOptionalU64 {
        present: true,
        value,
    }
}
fn color(value: CellColor) -> TuiColor {
    match value {
        CellColor::Default => TuiColor::default(),
        CellColor::Indexed(index) => TuiColor {
            kind: 1,
            index,
            ..Default::default()
        },
        CellColor::Rgb(red, green, blue) => TuiColor {
            kind: 2,
            red,
            green,
            blue,
            ..Default::default()
        },
    }
}
fn complete(mut result: TuiResult, storage: Storage) -> *mut TuiResult {
    result.private_data = Box::into_raw(Box::new(storage)).cast();
    Box::into_raw(Box::new(result))
}
pub(crate) fn error(error: TuiTestError) -> *mut TuiResult {
    let mut storage = Storage::default();
    let result = TuiResult {
        error_kind: error.kind.exit_code() as u32,
        error_message: storage.string(error.message),
        ..Default::default()
    };
    complete(result, storage)
}
pub(crate) fn sessions(names: Vec<String>) -> *mut TuiResult {
    let mut storage = Storage::default();
    for name in names {
        let value = storage.string(name);
        storage.strings.push(value);
    }
    let result = TuiResult {
        strings: storage.strings.as_ptr(),
        strings_len: storage.strings.len(),
        ..Default::default()
    };
    complete(result, storage)
}
pub(crate) fn operation(value: OperationResult) -> *mut TuiResult {
    let mut s = Storage::default();
    let mut r = TuiResult::default();
    match value {
        OperationResult::Unit => {}
        OperationResult::Open(v) => {
            r.open = TuiOpenResult {
                shell_pid: TuiOptionalU64 {
                    present: v.shell_pid.is_some(),
                    value: u64::from(v.shell_pid.unwrap_or_default()),
                },
                session: s.string(v.session),
                ready: v.ready,
                recording: s.string(v.recording),
            }
        }
        OperationResult::State(v) => {
            r.state = TuiState {
                session_shell: s.optional(v.session_shell),
                cols: v.cols,
                rows: v.rows,
                cursor: TuiCursor {
                    x: v.cursor.x,
                    y: v.cursor.y,
                },
                title: s.optional(v.title),
                cwd: s.optional(v.cwd),
                last_command: s.optional(v.last_command),
                last_exit: integer(v.last_exit),
                exited: integer(v.exited),
                ready: v.ready,
                bell_count: v.bell_count,
                timeouts: TuiTimeouts {
                    text: unsigned(v.timeouts.text),
                    idle: unsigned(v.timeouts.idle),
                    command: unsigned(v.timeouts.command),
                    exit: unsigned(v.timeouts.exit),
                    ready: unsigned(v.timeouts.ready),
                },
                text: s.string(v.text),
            }
        }
        OperationResult::Text(v)
        | OperationResult::Clipboard(v)
        | OperationResult::Recording(v) => r.text = s.string(v),
        OperationResult::Command(v)
        | OperationResult::Output(v)
        | OperationResult::Cwd(v)
        | OperationResult::Title(v) => r.text = s.optional(v),
        OperationResult::ExitCode(v) => r.exit_code = integer(v),
        OperationResult::Cursor(v) => r.cursor = TuiCursor { x: v.x, y: v.y },
        OperationResult::Size(v) => {
            r.size = TuiSize {
                cols: v.cols,
                rows: v.rows,
            }
        }
        OperationResult::BellCount(v) => r.number = v,
        OperationResult::PackedScreen(v) => {
            r.size = TuiSize {
                cols: v.cols,
                rows: v.rows,
            };
            r.text = s.bytes(v.utf8);
        }
        OperationResult::Cells(v) => {
            for c in v {
                let cell = TuiCell {
                    x: c.x,
                    y: c.y,
                    character: s.string(c.char),
                    fg: color(c.fg),
                    bg: color(c.bg),
                    bold: c.bold,
                    dim: c.dim,
                    italic: c.italic,
                    inverse: c.inverse,
                    invisible: c.invisible,
                    strike: c.strike,
                    blink: c.blink,
                    underline: c.underline,
                    underline_style: s.string(c.underline_style),
                    underline_color: color(c.underline_color),
                };
                s.cells.push(cell);
            }
            r.cells = s.cells.as_ptr();
            r.cells_len = s.cells.len();
        }
        OperationResult::Matches(v) => {
            for m in v {
                let spans = m
                    .spans
                    .into_iter()
                    .map(|v| TuiSpan {
                        row: v.row,
                        start: v.start,
                        end: v.end,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let value = TuiMatch {
                    text: s.string(m.text),
                    start: TuiPosition {
                        row: m.start.row,
                        column: m.start.column,
                    },
                    end: TuiPosition {
                        row: m.end.row,
                        column: m.end.column,
                    },
                    spans: spans.as_ptr(),
                    spans_len: spans.len(),
                };
                s.spans.push(spans);
                s.matches.push(value);
            }
            r.matches = s.matches.as_ptr();
            r.matches_len = s.matches.len();
        }
        OperationResult::BellEvents(v) => {
            s.bells = v
                .into_iter()
                .map(|v| TuiBellEvent {
                    sequence: v.sequence,
                    elapsed_ms: v.elapsed_ms,
                })
                .collect();
            r.bells = s.bells.as_ptr();
            r.bells_len = s.bells.len();
        }
        OperationResult::Snapshot(v) => {
            r.snapshot = match v {
                SnapshotResult::Passed => 0,
                SnapshotResult::Written => 1,
                SnapshotResult::Updated => 2,
            }
        }
        OperationResult::Screenshot(ScreenshotResult::Path(v) | ScreenshotResult::Text(v)) => {
            r.text = s.string(v)
        }
    }
    complete(r, s)
}
/// # Safety
/// result must be NULL or a live result returned by this library, freed once.
pub(crate) unsafe fn free(result: *mut TuiResult) {
    if !result.is_null() {
        let result = unsafe { Box::from_raw(result) };
        drop(unsafe { Box::from_raw(result.private_data.cast::<Storage>()) });
    }
}
