//! [`Emulator`] backend built on `@xterm/headless` running in QuickJS.
//!
//! The bundle and its host shim are embedded in the binary and evaluated into
//! a fresh QuickJS context per session, so this backend adds no runtime
//! dependency on Node or on anything installed on the machine.
//!
//! # Why the grid crosses the boundary packed
//!
//! Reading a cell means a call into JS, and an 80x30 screen is 2,400 of them
//! with ten property reads each. Walking the grid that way costs milliseconds
//! per poll. Instead [`shim.js`](../../../assets/xtermjs/shim.js) flattens a row
//! span into one string and one integer array, so a whole screen crosses in
//! two values and this module's job is decoding rather than traversal.
//!
//! # Threading
//!
//! [`Emulator`] is `Send` and the daemon moves the emulator between its reader
//! and request threads. `rquickjs`'s `parallel` feature makes `Runtime` and
//! `Context` `Send + Sync`, which is what lets this type be `Send` without
//! confining the interpreter to a thread of its own. It is emphatically not
//! `Sync`-in-spirit: every entry point below takes `&mut self`, so the daemon's
//! existing mutex is still what serializes access.

use std::sync::Mutex;

use alacritty_terminal::vte::{Parser, Perform};
use compact_str::{CompactString, ToCompactString};
use rquickjs::{Context, Ctx, Function, Object, Runtime};

use crate::event::BellTracker;
use crate::profile::{ColorSlot, Profile, Rgb};
use crate::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle, CONTINUATION};
use crate::terminal::emu::{CursorShape, Emulator};

const XTERM_BUNDLE: &str = include_str!("../../assets/xtermjs/xterm-headless.js");
const UNICODE11: &str = include_str!("../../assets/xtermjs/addon-unicode11.js");
const SHIM: &str = include_str!("../../assets/xtermjs/shim.js");

/// The unicode11 addon is UMD and publishes itself by *replacing*
/// `module.exports`, so it is lifted onto a global the shim can find. Reading
/// it back before `__boot` runs also leaves `exports.Terminal`, which the shim
/// set up, untouched.
const UNICODE11_CAPTURE: &str = "globalThis.__unicode11 = module.exports.Unicode11Addon;";

/// Ints per cell in the packed `meta` array, mirroring `pack()` in the shim.
const STRIDE: usize = 6;

/// Where the three dynamic colors sit in the flat table handed to the shim,
/// which continues past the 256 palette slots. The numbering is this
/// backend's own arrangement with its own shim and reaches no further.
const FOREGROUND: usize = 256;
const BACKGROUND: usize = 257;
const CURSOR: usize = 258;
const SLOTS: usize = 259;

/// Rows decoded per `pack` call. Bounds the size of the temporary JS array a
/// full-scrollback read builds; see [`XtermJsEmu::rows_in_range`].
const PACK_BATCH_ROWS: usize = 256;

/// Color-mode bits, packed alongside the SGR booleans in the `flags` int.
const FG_PALETTE: i32 = 256;
const FG_RGB: i32 = 512;
const BG_PALETTE: i32 = 1024;
const BG_RGB: i32 = 2048;
const UL_PALETTE: i32 = 4096;
const UL_RGB: i32 = 8192;

/// Decode one color slot. `mode` is the pair of bits that says how to read
/// `raw`; with neither set the cell uses the terminal default, which the cell
/// vocabulary spells as `None`.
fn color(raw: i32, flags: i32, palette_bit: i32, rgb_bit: i32) -> Option<Color> {
    if flags & palette_bit != 0 {
        Some(Color::from_index(raw as u8))
    } else if flags & rgb_bit != 0 {
        Some(Color::Rgb(
            ((raw >> 16) & 0xff) as u8,
            ((raw >> 8) & 0xff) as u8,
            (raw & 0xff) as u8,
        ))
    } else {
        None
    }
}

/// xterm.js's `UnderlineStyle`, which already folds "not underlined" into
/// `NONE` and a bare `SGR 4` into `SINGLE`, so no separate underline flag has
/// to be consulted here.
fn underline(raw: i32) -> UnderlineStyle {
    match raw {
        1 => UnderlineStyle::Single,
        2 => UnderlineStyle::Double,
        3 => UnderlineStyle::Curly,
        4 => UnderlineStyle::Dotted,
        5 => UnderlineStyle::Dashed,
        _ => UnderlineStyle::None,
    }
}

/// Pack a color into the `0xRRGGBB` int the shim speaks.
fn rgb_to_i32(c: Rgb) -> i32 {
    ((c.r as i32) << 16) | ((c.g as i32) << 8) | c.b as i32
}

fn attrs(flags: i32) -> Attrs {
    let mut a = Attrs::empty();
    for (bit, attr) in [
        (1, Attrs::BOLD),
        (2, Attrs::DIM),
        (4, Attrs::ITALIC),
        (8, Attrs::INVERSE),
        (16, Attrs::INVISIBLE),
        (32, Attrs::STRIKE),
        (64, Attrs::BLINK),
    ] {
        a.set(attr, flags & bit != 0);
    }
    a
}

struct BellListener {
    bells: BellTracker,
}

impl Perform for BellListener {
    fn execute(&mut self, byte: u8) {
        if byte == b'\x07' {
            self.bells.ring();
        }
    }
}

pub struct XtermJsEmu {
    // Held to keep the interpreter alive for as long as the context that runs
    // in it; nothing calls through it directly.
    _runtime: Runtime,
    ctx: Context,
    /// The size xterm.js actually applied, which is not always the size that
    /// was asked for: it clamps to a 2x1 minimum. Caching the *requested* size
    /// sheared the grid, because `pack` emits `term.cols` cells per row while
    /// the decoder chunks by this value.
    cols: u16,
    rows: u16,
    /// The settings this session was opened with. A program can shadow a
    /// color at runtime but never reach this, so a reset always has a value
    /// to restore.
    profile: Profile,
    /// The first JS exception this emulator hit, if any.
    ///
    /// The first one wins: once the engine has thrown, what follows is a
    /// consequence rather than a new fact, and the earliest message names the
    /// cause. Behind a lock because the reads that can fault take `&self`.
    fault: Mutex<Option<String>>,
    bell_parser: Parser,
    bell_listener: BellListener,
}

/// Render a failed JS call as a message worth reading.
///
/// rquickjs reports a thrown exception as a bare [`rquickjs::Error::Exception`]
/// and parks the value on the context, so the message and stack have to be
/// claimed with `catch` or the next call overwrites them.
fn describe(ctx: &Ctx<'_>, method: &str, error: rquickjs::Error) -> String {
    if !matches!(error, rquickjs::Error::Exception) {
        return format!("xterm.js: calling {method} failed: {error}");
    }
    let thrown = ctx.catch();
    let detail = match thrown.as_exception() {
        Some(exception) => match exception.stack() {
            Some(stack) if !stack.trim().is_empty() => {
                format!("{exception}\n{}", stack.trim_end())
            }
            _ => exception.to_string(),
        },
        None => format!("{thrown:?}"),
    };
    format!("xterm.js: {method} threw: {detail}")
}

impl XtermJsEmu {
    pub fn new(cols: u16, rows: u16, profile: &Profile) -> anyhow::Result<Self> {
        Self::with_bell_tracker(cols, rows, profile, BellTracker::default())
    }

    pub(crate) fn with_bell_tracker(
        cols: u16,
        rows: u16,
        profile: &Profile,
        bells: BellTracker,
    ) -> anyhow::Result<Self> {
        // The color every slot takes when no program has changed it. Resolved
        // here rather than in the shim so a query is answered with the same
        // value whichever backend a session runs on.
        let mut base = Vec::with_capacity(SLOTS);
        for index in 0..=255u8 {
            base.push(rgb_to_i32(profile.colors.rgb(index)));
        }
        base.push(rgb_to_i32(profile.colors.foreground));
        base.push(rgb_to_i32(profile.colors.background));
        base.push(rgb_to_i32(profile.colors.cursor));
        // A profile can ask for more scrollback than the count crossing into
        // JS can hold. Saturating keeps a deep request deep; the cast alone
        // would wrap it round to a shallow one.
        let scrollback = u32::try_from(profile.scrollback).unwrap_or(u32::MAX);
        let runtime = Runtime::new()?;
        let ctx = Context::full(&runtime)?;

        ctx.with(|ctx| -> anyhow::Result<()> {
            // Shim first: the bundle reads `process`/`exports` while it
            // evaluates, not just when the terminal is constructed.
            ctx.eval::<(), _>(SHIM)?;
            ctx.eval::<(), _>(XTERM_BUNDLE)?;
            ctx.eval::<(), _>(UNICODE11)?;
            ctx.eval::<(), _>(UNICODE11_CAPTURE)?;
            let boot: Function = ctx.globals().get("__boot")?;
            let emu: Object = boot.call((cols, rows, scrollback, base.clone()))?;
            ctx.globals().set("__emu", emu)?;
            Ok(())
        })?;

        let mut emu = XtermJsEmu {
            _runtime: runtime,
            ctx,
            cols,
            rows,
            profile: *profile,
            fault: Mutex::new(None),
            bell_parser: Parser::new(),
            bell_listener: BellListener { bells },
        };
        emu.sync_size();
        Ok(emu)
    }

    /// Adopt the size xterm.js settled on.
    fn sync_size(&mut self) {
        self.cols = self.call::<i32>("cols").clamp(0, u16::MAX as i32) as u16;
        self.rows = self.call::<i32>("rows").clamp(0, u16::MAX as i32) as u16;
    }

    /// Call a zero-argument method on the shim's emulator object.
    ///
    /// No `this` is threaded through: every method the shim returns is a
    /// closure over its own `term`, so the receiver is unused, and rquickjs
    /// would otherwise pass a `This` wrapper as the first positional argument.
    fn call<R>(&self, method: &str) -> R
    where
        R: for<'js> rquickjs::FromJs<'js> + Default,
    {
        self.call_or(method, R::default())
    }

    /// Like [`Self::call`], for a method whose failure should not read as
    /// `false`: a cursor is visible unless something says otherwise.
    fn call_or<R>(&self, method: &str, fallback: R) -> R
    where
        R: for<'js> rquickjs::FromJs<'js>,
    {
        self.invoke(method, |emu, _| emu.get::<_, Function>(method)?.call(()))
            .unwrap_or(fallback)
    }

    /// Call a one-argument method on the shim's emulator object.
    fn call_with<R>(&self, method: &str, arg: i32) -> R
    where
        R: for<'js> rquickjs::FromJs<'js> + Default,
    {
        self.invoke(method, |emu, _| {
            emu.get::<_, Function>(method)?.call((arg,))
        })
        .unwrap_or_default()
    }

    /// Run one call against the shim's emulator object, recording a thrown
    /// exception rather than reporting the call's own default in its place.
    ///
    /// A reader that only sees the fallback cannot tell an empty grid from a
    /// grid it failed to read, which is the distinction [`Self::fault`] keeps.
    fn invoke<R, F>(&self, method: &str, body: F) -> Option<R>
    where
        R: for<'js> rquickjs::FromJs<'js>,
        F: for<'js> FnOnce(&Object<'js>, &Ctx<'js>) -> rquickjs::Result<R>,
    {
        let result = self.ctx.with(|ctx| {
            let outcome = (|| -> rquickjs::Result<R> {
                let emu: Object = ctx.globals().get("__emu")?;
                body(&emu, &ctx)
            })();
            outcome.map_err(|error| describe(&ctx, method, error))
        });
        match result {
            Ok(value) => Some(value),
            Err(message) => {
                self.record_fault(message);
                None
            }
        }
    }

    /// Keep the first fault; see [`XtermJsEmu::fault`].
    fn record_fault(&self, message: String) {
        let mut fault = self
            .fault
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if fault.is_none() {
            *fault = Some(message);
        }
    }

    /// Decode one packed row span.
    ///
    /// Rows are read in batches rather than all at once. A full-scrollback
    /// grid is 5,000 rows, and packing it in one call builds a JS array of six
    /// boxed numbers per cell — 2.4 million of them — which lands above what
    /// QuickJS reclaims eagerly and below what makes it collect, so a poll loop
    /// calling `full_rows` grew the daemon by tens of megabytes per call.
    /// Batching keeps each allocation small enough to be collected between
    /// calls.
    fn rows_in_range(&self, full: bool) -> Vec<Vec<EmuCell>> {
        let (cols, _) = self.size();
        let cols = cols as usize;
        if cols == 0 {
            return Vec::new();
        }

        let span = self.ctx.with(|ctx| -> rquickjs::Result<(i32, i32)> {
            let emu: Object = ctx.globals().get("__emu")?;
            let start: i32 = emu.get::<_, Function>("start")?.call((full,))?;
            let end: i32 = emu.get::<_, Function>("end")?.call((full,))?;
            Ok((start, end))
        });
        let (start, end) = match span {
            Ok(span) => span,
            Err(_) => return Vec::new(),
        };

        let mut out = Vec::with_capacity((end - start).max(0) as usize);
        for batch in (start..end).step_by(PACK_BATCH_ROWS) {
            let batch_end = (batch + PACK_BATCH_ROWS as i32).min(end);
            let packed = self
                .ctx
                .with(|ctx| -> rquickjs::Result<(String, Vec<i32>)> {
                    let emu: Object = ctx.globals().get("__emu")?;
                    let packed: rquickjs::Array =
                        emu.get::<_, Function>("pack")?.call((batch, batch_end))?;
                    Ok((packed.get(0)?, packed.get(1)?))
                });
            let (chars, meta) = match packed {
                Ok(p) => p,
                Err(_) => return Vec::new(),
            };
            decode_into(&mut out, &chars, &meta, cols);
        }
        out
    }
}

/// Decode a packed batch into whole rows, appending to `out`.
fn decode_into(out: &mut Vec<Vec<EmuCell>>, chars: &str, meta: &[i32], cols: usize) {
    let mut cells = chars.split('\0');
    let mut row = Vec::with_capacity(cols);
    for m in meta.as_chunks::<STRIDE>().0 {
        let ch = cells.next().unwrap_or("");
        let (width, fg, bg, ul_color, ul_style, flags) = (m[0], m[1], m[2], m[3], m[4], m[5]);

        // Width alone does not identify a continuation. xterm.js also reports
        // width 0 for a genuine zero-width grapheme that had no base character
        // to combine with (a lone combining mark, ZWSP, ZWJ, or a variation
        // selector at the start of a row): that cell owns its column and holds
        // real text. Only an *empty* zero-width cell is the second column of a
        // double-width character. Reading width alone dropped the grapheme and
        // left the row one column short of the grid.
        let ch = if !ch.is_empty() {
            ch.to_compact_string()
        } else if width == 0 {
            CompactString::const_new(CONTINUATION)
        } else {
            CompactString::const_new(" ")
        };

        row.push(EmuCell {
            ch,
            fg: color(fg, flags, FG_PALETTE, FG_RGB),
            bg: color(bg, flags, BG_PALETTE, BG_RGB),
            underline: underline(ul_style),
            underline_color: color(ul_color, flags, UL_PALETTE, UL_RGB),
            attrs: attrs(flags),
        });

        if row.len() == cols {
            out.push(std::mem::replace(&mut row, Vec::with_capacity(cols)));
        }
    }
}

impl Emulator for XtermJsEmu {
    fn process(&mut self, bytes: &[u8]) {
        // VTE calls `execute` only for BEL in the ground state; BEL terminating
        // an OSC sequence is consumed by `osc_dispatch` instead.
        self.bell_parser.advance(&mut self.bell_listener, bytes);
        // Fed as bytes rather than as a string on purpose: xterm.js runs its
        // own incremental UTF-8 decoder over a byte array and carries a
        // partial sequence across calls, which is what keeps a multi-byte
        // character split across two PTY reads from being corrupted.
        self.invoke("feed", |emu, ctx| {
            let buf = rquickjs::TypedArray::<u8>::new(ctx.clone(), bytes)?;
            emu.get::<_, Function>("feed")?.call((buf,))
        })
        .unwrap_or(())
    }

    fn fault(&self) -> Option<String> {
        self.fault
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn ensure_bell_support(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn take_pending_writes(&mut self) -> Vec<u8> {
        self.call::<String>("takeReplies").into_bytes()
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.invoke("resize", |emu, _| {
            emu.get::<_, Function>("resize")?.call((cols, rows))
        })
        .unwrap_or(());
        // Read the size back rather than trusting the request: xterm.js clamps
        // to its 2x1 minimum, and recording a smaller size than the grid it
        // actually holds makes every later decode mis-chunk the rows.
        self.sync_size();
    }

    fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    fn cursor(&self) -> (u16, u16) {
        let x = self.call::<i32>("cursorX").max(0) as u16;
        let y = self.call::<i32>("cursorY").max(0) as u16;
        (
            x.min(self.cols.saturating_sub(1)),
            y.min(self.rows.saturating_sub(1)),
        )
    }

    fn title(&self) -> Option<String> {
        self.call::<Option<String>>("title")
            .filter(|title| !title.is_empty())
    }

    fn cursor_visible(&self) -> bool {
        self.call_or("cursorVisible", true)
    }

    fn cursor_shape(&self) -> CursorShape {
        match self.call::<String>("cursorShape").as_str() {
            "underline" => CursorShape::Underline,
            "bar" => CursorShape::Bar,
            // xterm.js also spells the beam `bar`, and a terminal that has
            // been told nothing draws a block.
            _ => CursorShape::Block,
        }
    }

    /// What a program set, else what the profile configured.
    ///
    /// The shim reports -1 for a slot no program has touched, which is what
    /// keeps a reset from having to remember the configured color separately.
    fn color(&self, slot: ColorSlot) -> Rgb {
        let colors = &self.profile.colors;
        let (index, configured) = match slot {
            ColorSlot::Indexed(index) => (index as usize, colors.rgb(index)),
            ColorSlot::Foreground => (FOREGROUND, colors.foreground),
            ColorSlot::Background => (BACKGROUND, colors.background),
            ColorSlot::Cursor => (CURSOR, colors.cursor),
        };
        match self.call_with::<i32>("colorOverride", index as i32) {
            set if set >= 0 => Rgb::new(
                ((set >> 16) & 0xff) as u8,
                ((set >> 8) & 0xff) as u8,
                (set & 0xff) as u8,
            ),
            _ => configured,
        }
    }

    fn viewable_rows(&self) -> Vec<Vec<EmuCell>> {
        self.rows_in_range(false)
    }

    fn full_rows(&self) -> Vec<Vec<EmuCell>> {
        self.rows_in_range(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JS exception has to reach the caller, not read back as an empty grid.
    ///
    /// Everything below `process` returns a value rather than a `Result`, so
    /// before this a thrown exception left the grid frozen at whatever it last
    /// held and every later read reported that stale grid as fact.
    #[test]
    fn a_thrown_exception_is_reported_rather_than_swallowed() {
        let mut emu = XtermJsEmu::new(10, 2, &Profile::default()).expect("create emulator");
        emu.process(b"ok");
        assert!(emu.fault().is_none(), "a healthy emulator has no fault");

        // Break `feed` the way a bundle upgrade moving a private API would.
        emu.ctx
            .with(|ctx| -> rquickjs::Result<()> {
                let emu: Object = ctx.globals().get("__emu")?;
                let thrower: Function =
                    ctx.eval("(function () { throw new Error('feed exploded'); })")?;
                emu.set("feed", thrower)
            })
            .expect("replace feed");

        emu.process(b"more");
        let fault = emu.fault().expect("the exception is recorded");
        assert!(
            fault.contains("feed exploded"),
            "the fault carries the JS message: {fault}"
        );

        // The first fault wins: a later failure does not overwrite the message
        // that named the cause.
        emu.resize(20, 4);
        assert_eq!(
            emu.fault().as_deref(),
            Some(fault.as_str()),
            "the first fault is kept"
        );
    }

    #[test]
    fn multiple_bells_in_one_chunk_are_counted_individually() {
        let bells = BellTracker::default();
        let mut emulator =
            XtermJsEmu::with_bell_tracker(80, 24, &Profile::default(), bells.clone())
                .expect("create emulator");

        emulator.process(b"\x07\x07");

        assert_eq!(bells.count(), 2);
        assert_eq!(bells.sequence(), 2);
    }

    #[test]
    fn an_osc_bell_terminator_does_not_ring_the_terminal_bell() {
        let bells = BellTracker::default();
        let mut emulator =
            XtermJsEmu::with_bell_tracker(80, 24, &Profile::default(), bells.clone())
                .expect("create emulator");

        emulator.process(b"\x1b]0;window");
        emulator.process(b" title\x07");
        assert_eq!(bells.count(), 0);

        emulator.process(b"\x07");
        assert_eq!(bells.count(), 1);
    }

    crate::emulator_conformance_tests!(
        |cols, rows, profile| {
            Box::new(XtermJsEmu::new(cols, rows, profile).expect("create xterm.js emulator"))
        },
        crate::terminal::conformance::Divergences {
            // xterm.js keeps a cell's underline color in an extended-attribute
            // record it allocates only for a cell that has an underline style,
            // so `SGR 58` alone is not readable back off the cell. Verified
            // against the bundle rather than inferred: the cell reports
            // `isAttributeDefault()`, and the color is absent from the line's
            // extended attributes while remaining in the current SGR state.
            underline_color_needs_a_style: true,
            bell_events_unsupported: false,
        }
    );
}
