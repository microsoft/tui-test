use crate::types::*;
use tui_test::api::*;

include!("output_generated.rs");

#[derive(Default)]
struct Storage {
    bytes: Vec<Box<[u8]>>,
    cells: Vec<TuiCell>,
    matches: Vec<TuiMatch>,
    spans: Vec<Box<[TuiSpan]>>,
    bells: Vec<TuiBellEvent>,
    strings: Vec<TuiString>,
    modes: Vec<TuiMode>,
    pairs: Vec<TuiPair>,
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
    fn modes(
        &mut self,
        values: std::collections::BTreeMap<String, bool>,
    ) -> (*const TuiMode, usize) {
        for (name, enabled) in values {
            let name = self.string(name);
            self.modes.push(TuiMode { name, enabled });
        }
        (self.modes.as_ptr(), self.modes.len())
    }
    fn palette(
        &mut self,
        values: std::collections::BTreeMap<u8, String>,
    ) -> (*const TuiPair, usize) {
        for (index, color) in values {
            let key = self.string(index.to_string());
            let value = self.string(color);
            self.pairs.push(TuiPair { key, value });
        }
        (self.pairs.as_ptr(), self.pairs.len())
    }
    fn spans(&mut self, values: Vec<TextSpan>) -> (*const TuiSpan, usize) {
        let spans = values
            .into_iter()
            .map(|v| text_span(self, v))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let result = (spans.as_ptr(), spans.len());
        self.spans.push(spans);
        result
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
    let details = error
        .details
        .as_deref()
        .and_then(|value| serde_json::to_vec(value).ok())
        .map(|value| storage.bytes(value))
        .unwrap_or_default();
    let artifact = error
        .artifact
        .as_deref()
        .and_then(|value| serde_json::to_vec(value).ok())
        .map(|value| storage.bytes(value))
        .unwrap_or_default();
    let result = TuiResult {
        error_kind: error.kind.exit_code() as u32,
        error_message: storage.string(error.message),
        error_details: details,
        error_artifact: artifact,
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
        OperationResult::Viewport(v) => r.viewport = viewport(&mut s, v),
        OperationResult::Open(v) => r.open = open_value(&mut s, v),
        OperationResult::State(v) => r.state = state(&mut s, *v),
        OperationResult::Text(v)
        | OperationResult::Clipboard(v)
        | OperationResult::Recording(v) => r.text = s.string(v),
        OperationResult::Command(v)
        | OperationResult::Output(v)
        | OperationResult::Cwd(v)
        | OperationResult::Title(v) => r.text = s.optional(v),
        OperationResult::ExitCode(v) => r.exit_code = integer(v),
        OperationResult::Cursor(v) => r.cursor = cursor(&mut s, v),
        OperationResult::Modes(v) => {
            for (name, enabled) in v {
                let name = s.string(name);
                s.modes.push(TuiMode { name, enabled });
            }
        }
        OperationResult::Colors(v) => {
            for (index, color) in v.palette {
                let key = s.string(index.to_string());
                let value = s.string(color);
                s.pairs.push(TuiPair { key, value });
            }
        }
        OperationResult::Size(v) => r.size = size(&mut s, v),
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
                let cell = cell(&mut s, c);
                s.cells.push(cell);
            }
            r.cells = s.cells.as_ptr();
            r.cells_len = s.cells.len();
        }
        OperationResult::Matches(v) => {
            for m in v {
                let value = text_match(&mut s, m);
                s.matches.push(value);
            }
            r.matches = s.matches.as_ptr();
            r.matches_len = s.matches.len();
        }
        OperationResult::BellEvents(v) => {
            s.bells = v.into_iter().map(|v| bell_event(&mut s, v)).collect();
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
