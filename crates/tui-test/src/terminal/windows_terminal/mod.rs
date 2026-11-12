//! Headless Windows Terminal TerminalCore, confined to its owning worker thread.
//!
//! The bridge links a pinned, unmodified upstream core. No window, ConPTY
//! emulation, system clipboard, or substitute VT parser is involved.

use std::ptr::NonNull;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use anyhow::{anyhow, bail, ensure, Context, Result};
use compact_str::CompactString;

use crate::api::{ScrollTarget, Viewport};
use crate::event::BellTracker;
use crate::profile::{ColorSlot, Profile, Rgb};
use crate::terminal::cell::{Attrs, Color, EmuCell, LinkCache, UnderlineStyle};
use crate::terminal::emu::{
    ClipboardType, ClipboardValidator, ColorTable, CursorShape, Emulator, KeyboardMode,
    TerminalMode,
};

mod ffi;

#[derive(Default)]
struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    fn decode(&mut self, bytes: &[u8]) -> Vec<u16> {
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::new();
        let mut consumed = 0;
        while consumed < self.pending.len() {
            match std::str::from_utf8(&self.pending[consumed..]) {
                Ok(text) => {
                    output.extend(text.encode_utf16());
                    consumed = self.pending.len();
                }
                Err(error) => {
                    let end = consumed + error.valid_up_to();
                    let valid = std::str::from_utf8(&self.pending[consumed..end])
                        .expect("UTF-8 validator returned a valid prefix");
                    output.extend(valid.encode_utf16());
                    consumed = end;
                    match error.error_len() {
                        Some(length) => {
                            output.push(0xfffd);
                            consumed += length;
                        }
                        None => break,
                    }
                }
            }
        }
        self.pending.drain(..consumed);
        output
    }
}

fn rgb(color: u32) -> Rgb {
    Rgb::new(color as u8, (color >> 8) as u8, (color >> 16) as u8)
}

fn colorref(color: Rgb) -> u32 {
    u32::from(color.r) | (u32::from(color.g) << 8) | (u32::from(color.b) << 16)
}

fn cell_color(value: u32) -> Result<Option<Color>> {
    match value >> 24 {
        0 if value == 0 => Ok(None),
        1 if value & 0x00ff_ffff <= 255 => Ok(Some(Color::from_index(value as u8))),
        2 => {
            let color = rgb(value);
            Ok(Some(Color::Rgb(color.r, color.g, color.b)))
        }
        _ => bail!("invalid TerminalCore cell color {value:#x}"),
    }
}

struct Snapshot(NonNull<ffi::Snapshot>);

impl Snapshot {
    fn raw(&self) -> &ffi::Snapshot {
        // The native allocation is immutable and lives until this wrapper drops.
        unsafe { self.0.as_ref() }
    }

    fn text(&self, span: ffi::Span) -> Result<String> {
        let raw = self.raw();
        let end = span
            .offset
            .checked_add(span.length)
            .context("UTF-16 span overflow")?;
        ensure!(
            end <= raw.string_count,
            "TerminalCore UTF-16 span is out of bounds"
        );
        if span.length == 0 {
            return Ok(String::new());
        }
        ensure!(
            !raw.strings.is_null(),
            "TerminalCore returned a null string buffer"
        );
        ensure!(
            end <= isize::MAX as usize / 2,
            "TerminalCore string buffer is too large"
        );
        // Spans were checked against the native-owned allocation above.
        let text = unsafe { std::slice::from_raw_parts(raw.strings.add(span.offset), span.length) };
        Ok(String::from_utf16_lossy(text))
    }

    fn rows(&self) -> Result<Vec<Vec<EmuCell>>> {
        let raw = self.raw();
        let cols = raw.state.cols as usize;
        ensure!(
            cols > 0 && raw.cell_count.is_multiple_of(cols),
            "TerminalCore returned a ragged grid"
        );
        if raw.cell_count == 0 {
            return Ok(Vec::new());
        }
        ensure!(
            !raw.cells.is_null(),
            "TerminalCore returned a null cell buffer"
        );
        ensure!(
            raw.cell_count <= isize::MAX as usize / std::mem::size_of::<ffi::Cell>(),
            "TerminalCore cell buffer is too large"
        );
        // The native library owns this buffer until wt_snapshot_free.
        let cells = unsafe { std::slice::from_raw_parts(raw.cells, raw.cell_count) };
        let mut links = LinkCache::default();
        cells
            .chunks_exact(cols)
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        let uri = self.text(cell.uri)?;
                        let id = self.text(cell.link_id)?;
                        let attrs = u8::try_from(cell.attrs)
                            .ok()
                            .and_then(Attrs::from_bits)
                            .context("invalid TerminalCore SGR attributes")?;
                        Ok(EmuCell {
                            ch: CompactString::from(self.text(cell.text)?),
                            fg: cell_color(cell.foreground)?,
                            bg: cell_color(cell.background)?,
                            underline_color: cell_color(cell.underline_color)?,
                            underline: match cell.underline_style {
                                0 => UnderlineStyle::None,
                                1 => UnderlineStyle::Single,
                                2 => UnderlineStyle::Double,
                                3 => UnderlineStyle::Curly,
                                4 => UnderlineStyle::Dotted,
                                5 => UnderlineStyle::Dashed,
                                other => bail!("invalid TerminalCore underline style {other}"),
                            },
                            attrs,
                            hyperlink: (!uri.is_empty())
                                .then(|| links.get((!id.is_empty()).then_some(id.as_str()), &uri)),
                        })
                    })
                    .collect()
            })
            .collect()
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        unsafe { ffi::wt_snapshot_free(self.0.as_ptr()) };
    }
}

struct Core {
    terminal: NonNull<ffi::Terminal>,
    decoder: Utf8Decoder,
    clipboard_validator: ClipboardValidator,
    bells: BellTracker,
    fault: Option<String>,
}

impl Core {
    fn new(cols: u16, rows: u16, profile: Profile, bells: BellTracker) -> Result<Self> {
        let colors = profile.colors;
        let config = ffi::Config {
            abi_version: ffi::ABI_VERSION,
            cols,
            rows,
            scrollback: u32::try_from(profile.scrollback)
                .context("TerminalCore scrollback overflow")?,
            colors: std::array::from_fn(|index| {
                colorref(match index {
                    0 => colors.foreground,
                    1 => colors.background,
                    2 => colors.cursor,
                    _ => colors.rgb((index - 3) as u8),
                })
            }),
        };
        let mut terminal = std::ptr::null_mut();
        let mut error = ffi::Error::default();
        let status = unsafe { ffi::wt_create(&config, &mut terminal, &mut error) };
        error.check(status)?;
        let core = Self {
            terminal: NonNull::new(terminal).context("TerminalCore returned a null terminal")?,
            decoder: Utf8Decoder::default(),
            clipboard_validator: ClipboardValidator::new(),
            bells,
            fault: None,
        };
        let state = core.read(0)?.raw().state;
        ensure!(
            (state.cols, state.rows) == (u32::from(cols), u32::from(rows)),
            "TerminalCore changed the requested dimensions"
        );
        Ok(core)
    }

    fn read(&self, rows: u32) -> Result<Snapshot> {
        let mut raw = std::ptr::null_mut();
        let mut error = ffi::Error::default();
        let status = unsafe { ffi::wt_read(self.terminal.as_ptr(), rows, &mut raw, &mut error) };
        error.check(status)?;
        let snapshot =
            Snapshot(NonNull::new(raw).context("TerminalCore returned a null snapshot")?);
        ensure!(
            snapshot.raw().abi_version == ffi::ABI_VERSION,
            "TerminalCore snapshot ABI mismatch"
        );
        let state = snapshot.raw().state;
        ensure!(
            state.cols > 0
                && state.rows > 0
                && state.cols <= u16::MAX as u32
                && state.rows <= u16::MAX as u32,
            "invalid TerminalCore grid dimensions"
        );
        ensure!(
            state.offset <= state.history,
            "invalid TerminalCore viewport offset"
        );
        let expected = match rows {
            0 => 0,
            1 | 2 => state.rows as usize * state.cols as usize,
            3 => (state.history as usize + state.rows as usize) * state.cols as usize,
            _ => bail!("invalid row selection"),
        };
        ensure!(
            snapshot.raw().cell_count == expected,
            "TerminalCore returned an incomplete grid"
        );
        Ok(snapshot)
    }

    fn process(&mut self, bytes: &[u8]) {
        if self.fault.is_some() {
            return;
        }
        self.clipboard_validator.process(bytes);
        let text = self.decoder.decode(bytes);
        let mut bells = 0;
        let mut error = ffi::Error::default();
        let status = unsafe {
            ffi::wt_write(
                self.terminal.as_ptr(),
                text.as_ptr(),
                text.len(),
                &mut bells,
                &mut error,
            )
        };
        if let Err(error) = error.check(status) {
            self.fault = Some(format!("{error:#}"));
        } else {
            for _ in 0..bells {
                self.bells.ring();
            }
        }
    }

    fn take_output(&self) -> Result<Vec<u8>> {
        let mut raw = std::ptr::null_mut();
        let mut error = ffi::Error::default();
        let status = unsafe { ffi::wt_take_output(self.terminal.as_ptr(), &mut raw, &mut error) };
        error.check(status)?;
        let raw = NonNull::new(raw).context("TerminalCore returned a null output")?;
        struct OwnedOutput(NonNull<ffi::Output>);
        impl Drop for OwnedOutput {
            fn drop(&mut self) {
                unsafe { ffi::wt_output_free(self.0.as_ptr()) };
            }
        }
        let output = OwnedOutput(raw);
        let raw = unsafe { output.0.as_ref() };
        if raw.length == 0 {
            return Ok(Vec::new());
        }
        ensure!(
            !raw.text.is_null() && raw.length <= isize::MAX as usize / 2,
            "invalid TerminalCore output"
        );
        let text = unsafe { std::slice::from_raw_parts(raw.text, raw.length) };
        Ok(String::from_utf16_lossy(text).into_bytes())
    }

    fn mutate(&mut self, operation: impl FnOnce(*mut ffi::Terminal, &mut ffi::Error) -> i32) {
        if self.fault.is_some() {
            return;
        }
        let mut error = ffi::Error::default();
        let status = operation(self.terminal.as_ptr(), &mut error);
        if let Err(error) = error.check(status) {
            self.fault = Some(format!("{error:#}"));
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe { ffi::wt_destroy(self.terminal.as_ptr()) };
    }
}

type Job = Box<dyn FnOnce(&mut Core) + Send + 'static>;

pub struct WindowsTerminalEmu {
    jobs: Option<Sender<Job>>,
    worker: Option<JoinHandle<()>>,
}

impl WindowsTerminalEmu {
    pub fn new(cols: u16, rows: u16, profile: &Profile) -> Result<Self> {
        Self::with_bell_tracker(cols, rows, profile, BellTracker::default())
    }

    pub(crate) fn with_bell_tracker(
        cols: u16,
        rows: u16,
        profile: &Profile,
        bells: BellTracker,
    ) -> Result<Self> {
        let profile = *profile;
        let (jobs, receiver) = mpsc::channel::<Job>();
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("tui-test-windows-terminal".to_string())
            .spawn(move || match Core::new(cols, rows, profile, bells) {
                Ok(mut core) => {
                    let _ = ready.send(Ok(()));
                    while let Ok(job) = receiver.recv() {
                        job(&mut core);
                    }
                }
                Err(error) => {
                    let _ = ready.send(Err(format!("{error:#}")));
                }
            })
            .context("spawning TerminalCore worker")?;
        match started.recv().context("starting TerminalCore worker")? {
            Ok(()) => Ok(Self {
                jobs: Some(jobs),
                worker: Some(worker),
            }),
            Err(error) => {
                let _ = worker.join();
                Err(anyhow!(error))
            }
        }
    }

    fn call<T: Send + 'static>(&self, job: impl FnOnce(&mut Core) -> T + Send + 'static) -> T {
        let (reply, response) = mpsc::sync_channel(1);
        self.jobs
            .as_ref()
            .expect("TerminalCore worker is shutting down")
            .send(Box::new(move |core| {
                let _ = reply.send(job(core));
            }))
            .unwrap_or_else(|_| panic!("TerminalCore worker stopped"));
        response
            .recv()
            .unwrap_or_else(|_| panic!("TerminalCore worker stopped"))
    }

    fn read<T: Send + 'static>(
        &self,
        rows: u32,
        read: impl FnOnce(&Snapshot) -> Result<T> + Send + 'static,
    ) -> T {
        self.call(move |core| core.read(rows).and_then(|snapshot| read(&snapshot)))
            .unwrap_or_else(|error| panic!("TerminalCore state read failed: {error:#}"))
    }
}

impl Drop for WindowsTerminalEmu {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Emulator for WindowsTerminalEmu {
    fn process(&mut self, bytes: &[u8]) {
        let bytes = bytes.to_vec();
        self.call(move |core| core.process(&bytes));
    }

    fn fault(&self) -> Option<String> {
        self.call(|core| {
            core.fault
                .clone()
                .or_else(|| core.clipboard_validator.fault())
        })
    }

    fn take_pending_writes(&mut self) -> Vec<u8> {
        self.call(|core| core.take_output())
            .unwrap_or_else(|error| panic!("TerminalCore output read failed: {error:#}"))
    }

    fn clipboard(&self, clipboard: ClipboardType) -> Result<String> {
        ensure!(
            clipboard == ClipboardType::Clipboard,
            "TerminalCore does not distinguish OSC 52 selections"
        );
        self.call(|core| {
            let snapshot = core.read(0)?;
            snapshot.text(snapshot.raw().clipboard)
        })
    }

    fn clipboard_revision(&self, clipboard: ClipboardType) -> Result<u64> {
        ensure!(
            clipboard == ClipboardType::Clipboard,
            "TerminalCore does not distinguish OSC 52 selections"
        );
        self.call(|core| Ok(core.read(0)?.raw().clipboard_revision))
    }

    fn keyboard_mode(&self) -> KeyboardMode {
        self.read(0, |snapshot| {
            u8::try_from(snapshot.raw().state.kitty_flags)
                .ok()
                .and_then(KeyboardMode::from_bits)
                .context("invalid TerminalCore Kitty keyboard flags")
        })
    }

    fn mode(&self, mode: TerminalMode) -> bool {
        self.read(0, move |snapshot| {
            let bit = TerminalMode::ALL
                .iter()
                .position(|&value| value == mode)
                .unwrap();
            Ok(snapshot.raw().state.modes & (1 << bit) != 0)
        })
    }

    fn cursor_key_application(&self) -> bool {
        self.mode(TerminalMode::ApplicationCursorKeys)
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.call(move |core| {
            core.mutate(|terminal, error| unsafe { ffi::wt_resize(terminal, cols, rows, error) });
        });
    }

    fn scroll(&mut self, target: ScrollTarget) {
        self.call(move |core| {
            let state = match core.read(0) {
                Ok(snapshot) => snapshot.raw().state,
                Err(error) => {
                    core.fault = Some(format!("{error:#}"));
                    return;
                }
            };
            let offset = match target {
                ScrollTarget::Top => state.history,
                ScrollTarget::Bottom => 0,
                ScrollTarget::Lines(lines) => (i64::from(state.offset) + i64::from(lines))
                    .clamp(0, i64::from(state.history))
                    as u32,
            };
            core.mutate(|terminal, error| unsafe { ffi::wt_scroll(terminal, offset, error) });
        });
    }

    fn viewport(&self) -> Viewport {
        self.read(0, |snapshot| {
            Ok(Viewport {
                offset: snapshot.raw().state.offset as usize,
                history: snapshot.raw().state.history as usize,
            })
        })
    }

    fn size(&self) -> (u16, u16) {
        self.read(0, |snapshot| {
            Ok((
                snapshot.raw().state.cols as u16,
                snapshot.raw().state.rows as u16,
            ))
        })
    }

    fn cursor(&self) -> (u16, u16) {
        self.read(0, |snapshot| {
            Ok((
                snapshot.raw().state.cursor_x as u16,
                snapshot.raw().state.cursor_y as u16,
            ))
        })
    }

    fn title(&self) -> Option<String> {
        self.read(0, |snapshot| {
            let title = snapshot.text(snapshot.raw().title)?;
            Ok((!title.is_empty()).then_some(title))
        })
    }

    fn cursor_shape(&self) -> CursorShape {
        self.read(0, |snapshot| match snapshot.raw().state.cursor_shape {
            0 => Ok(CursorShape::Block),
            1 => Ok(CursorShape::Underline),
            2 => Ok(CursorShape::Bar),
            other => bail!("invalid TerminalCore cursor shape {other}"),
        })
    }

    fn viewable_rows(&self) -> Vec<Vec<EmuCell>> {
        self.read(1, Snapshot::rows)
    }

    fn active_rows(&self) -> Vec<Vec<EmuCell>> {
        self.read(2, Snapshot::rows)
    }

    fn full_rows(&self) -> Vec<Vec<EmuCell>> {
        self.read(3, Snapshot::rows)
    }

    fn color(&self, slot: ColorSlot) -> Rgb {
        let index = match slot {
            ColorSlot::Foreground => 0,
            ColorSlot::Background => 1,
            ColorSlot::Cursor => 2,
            ColorSlot::Indexed(index) => usize::from(index) + 3,
        };
        self.read(0, move |snapshot| {
            Ok(rgb(snapshot.raw().state.colors[index]))
        })
    }

    fn colors(&self) -> ColorTable {
        self.read(0, |snapshot| {
            let colors = snapshot.raw().state.colors;
            Ok(ColorTable {
                foreground: rgb(colors[0]),
                background: rgb(colors[1]),
                cursor: rgb(colors[2]),
                palette: std::array::from_fn(|index| rgb(colors[index + 3])),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::emulator_conformance_tests!(
        |cols, rows, profile| { Box::new(WindowsTerminalEmu::new(cols, rows, profile).unwrap()) },
        &[crate::terminal::conformance::Divergence::NoLegacyAlternateScreen]
    );

    #[test]
    fn native_hyperlink_hash_suffix_is_not_part_of_the_users_id() {
        let mut emulator = WindowsTerminalEmu::new(12, 3, &Profile::default()).unwrap();
        emulator.process(b"\x1b]8;id=with%percent;https://example.com/a\x1b\\A\x1b]8;;\x1b\\");
        emulator.process(b"\x1b]8;id=with%percent;https://example.com/b\x1b\\B\x1b]8;;\x1b\\");
        let row = &emulator.active_rows()[0];
        for cell in &row[..2] {
            assert_eq!(
                cell.hyperlink.as_ref().unwrap().id.as_deref(),
                Some("with%percent")
            );
        }
        assert_ne!(row[0].uri(), row[1].uri());
    }

    #[test]
    fn native_colon_underline_colors_map_without_changing_the_foreground() {
        let mut emulator = WindowsTerminalEmu::new(12, 3, &Profile::default()).unwrap();
        emulator.process(b"\x1b[31;4:3;58:2::12:34:56mA\x1b[24mB\x1b[59mC");
        let row = &emulator.active_rows()[0];
        assert_eq!(row[0].underline, UnderlineStyle::Curly);
        assert_eq!(row[0].fg, Some(Color::from_index(1)));
        assert_eq!(row[0].underline_color, Some(Color::Rgb(12, 34, 56)));
        assert_eq!(row[1].underline, UnderlineStyle::None);
        assert_eq!(row[1].underline_color, Some(Color::Rgb(12, 34, 56)));
        assert_eq!(row[2].underline_color, None);
    }

    #[test]
    fn native_resize_failures_are_sticky_faults() {
        let mut emulator = WindowsTerminalEmu::new(12, 3, &Profile::default()).unwrap();
        emulator.process(b"before");
        emulator.resize(0, 3);
        assert!(emulator.fault().unwrap().contains("dimensions"));
        emulator.process(b"after");
        assert_eq!(conformance_text(&emulator.active_rows())[0], "before");
        assert_eq!(emulator.size(), (12, 3));
    }

    #[test]
    fn native_queries_do_not_disturb_a_split_escape_or_generate_input() {
        let mut emulator = WindowsTerminalEmu::new(12, 3, &Profile::default()).unwrap();
        emulator.process(b"\x1b[38;2;12");
        assert!(!emulator.mode(TerminalMode::Insert));
        assert!(emulator.take_pending_writes().is_empty());
        emulator.process(b";34;56mX");
        assert_eq!(
            emulator.active_rows()[0][0].fg,
            Some(Color::Rgb(12, 34, 56))
        );
        emulator.process(b"\x1b[?200");
        assert!(!emulator.mode(TerminalMode::BracketedPaste));
        emulator.process(b"4h");
        assert!(emulator.mode(TerminalMode::BracketedPaste));
        assert!(emulator.take_pending_writes().is_empty());
    }

    #[test]
    fn invalid_native_dimensions_and_abi_are_errors_not_null_handles() {
        assert!(WindowsTerminalEmu::new(0, 3, &Profile::default()).is_err());
        assert!(WindowsTerminalEmu::new(u16::MAX, 3, &Profile::default()).is_err());
        let config = ffi::Config {
            abi_version: ffi::ABI_VERSION + 1,
            cols: 10,
            rows: 3,
            scrollback: 0,
            colors: [0; ffi::COLOR_COUNT],
        };
        let mut raw = std::ptr::null_mut();
        let mut error = ffi::Error::default();
        let status = unsafe { ffi::wt_create(&config, &mut raw, &mut error) };
        assert!(error.check(status).unwrap_err().to_string().contains("ABI"));
        assert!(raw.is_null());
    }

    #[test]
    fn utf8_decoder_preserves_every_split_and_replaces_invalid_bytes() {
        let text = "aé你🙂e\u{301}";
        for split in 0..=text.len() {
            let mut decoder = Utf8Decoder::default();
            let mut decoded = decoder.decode(&text.as_bytes()[..split]);
            decoded.extend(decoder.decode(&text.as_bytes()[split..]));
            assert_eq!(String::from_utf16(&decoded).unwrap(), text, "split {split}");
        }
        let mut decoder = Utf8Decoder::default();
        assert_eq!(
            String::from_utf16(&decoder.decode(b"a\xffb")).unwrap(),
            "a�b"
        );
    }

    #[test]
    fn bells_and_clipboard_are_session_local() {
        let bells = BellTracker::default();
        let mut first =
            WindowsTerminalEmu::with_bell_tracker(12, 3, &Profile::default(), bells.clone())
                .unwrap();
        let second = WindowsTerminalEmu::new(12, 3, &Profile::default()).unwrap();
        first.process(b"\x07\x1b]52;c;aGVsbG8=\x1b\\\x07");
        assert_eq!(bells.count(), 2);
        assert_eq!(first.clipboard(ClipboardType::Clipboard).unwrap(), "hello");
        assert_eq!(second.clipboard(ClipboardType::Clipboard).unwrap(), "");
        assert_eq!(first.fault(), None);
    }

    #[test]
    fn moving_the_handle_between_threads_keeps_native_state_on_its_worker() {
        let emulator = WindowsTerminalEmu::new(12, 3, &Profile::default()).unwrap();
        let emulator = thread::spawn(move || {
            let mut emulator = emulator;
            emulator.process("🙂".as_bytes());
            assert_eq!(emulator.active_rows()[0][0].ch, "🙂");
            emulator
        })
        .join()
        .unwrap();
        assert_eq!(emulator.cursor(), (2, 0));
    }
}
