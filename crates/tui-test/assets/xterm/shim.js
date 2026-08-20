// Host shim + Rust-facing glue for the vendored `@xterm/headless` bundle.
//
// Two jobs:
//
// 1. Stand in for the handful of browser/Node globals the bundle reaches for.
//    `process.title` is the important one: xterm.js branches on
//    `typeof process !== 'undefined' && 'title' in process` to set `isNode`,
//    and every `navigator` access sits on the other side of that branch, so
//    defining it means no user-agent sniffing ever runs.
//
// 2. Make writing synchronous. `Terminal.write()` is async: it queues the
//    chunk and drains it from a `setTimeout` callback, so the grid is still
//    empty when it returns. `Emulator::process` has to have the grid ready
//    when it returns, so timers are collected into a queue that `feed()`
//    drains to empty before it hands control back to Rust. The drain is
//    ordinary FIFO, which is all xterm.js needs: it only ever schedules its
//    own continuation.
//
// `performance.now()` returning a constant is deliberate, not a stub: the
// write loop yields to a fresh timer once it has spent 12ms on a chunk, and a
// frozen clock means it never does, so one `feed()` always consumes the whole
// chunk instead of leaving a tail for the next drain.

globalThis.process = { title: 'tui-test' };
globalThis.performance = { now: function () { return 0; } };
globalThis.console = {
  log: function () {}, warn: function () {}, error: function () {},
  debug: function () {}, info: function () {}, trace: function () {},
};

var __timers = [];
globalThis.setTimeout = function (fn) { return __timers.push(fn); };
globalThis.clearTimeout = function () {};
globalThis.setInterval = function () { return 0; };
globalThis.clearInterval = function () {};
globalThis.queueMicrotask = function (fn) { __timers.push(fn); };

// The bundle is UMD and assigns to `exports`.
globalThis.exports = {};
globalThis.module = { exports: globalThis.exports };

globalThis.__boot = function (cols, rows, scrollback, base) {
  var term = new exports.Terminal({
    cols: cols,
    rows: rows,
    scrollback: scrollback,
    // `getUnderlineStyle`, `getUnderlineColor` and `getNullCell` are proposed
    // API; without this every one of them throws.
    allowProposedApi: true,

    // `CSI 22 t` pushes the window title and `CSI 23 t` pops it, which is how
    // a shell puts back the title it had before running a command. xterm.js
    // implements both but leaves them off by default, and the rest of the
    // window operations stay off: this terminal has no window to resize,
    // move, or report the position of.
    windowOptions: { pushTitle: true, popTitle: true },
  });

  // The headless bundle ships only the Unicode 6 width tables, which call
  // every astral emoji one column wide. alacritty measures them as two, so
  // without this a line containing an emoji puts every following cell in a
  // different column on the two backends, moving what `cells`, the locator,
  // and the SVG renderer report. The Unicode 11 provider restores the pair.
  if (typeof globalThis.__unicode11 === 'function') {
    term.loadAddon(new globalThis.__unicode11());
    term.unicode.activeVersion = '11';
  }

  // Replies the terminal wants sent back up the PTY (DA, CPR, and friends).
  var replies = [];
  term.onData(function (d) { replies.push(d); });

  var title = null;
  term.onTitleChange(function (t) { title = t ? t : null; });


  // Colors a program set at runtime, keyed by the same slot numbering the
  // Rust side uses: 0-255 palette, then foreground, background, cursor. Only
  // what a program actually set lives here, so clearing an entry restores
  // whatever the profile configured without having to remember it twice.
  var FG = 256, BG = 257, CURSOR = 258;
  var overrides = {};

  // `base[i]` is the color slot `i` resolves to when no program has changed
  // it: the profile for the sixteen ANSI slots and the three dynamic ones,
  // and the standard xterm table for the rest. It is computed on the Rust
  // side so both backends answer a query with the same value.
  function resolved(slot) {
    var v = overrides[slot];
    return v === undefined ? base[slot] : v;
  }

  // xterm.js hands an OSC handler its payload but not the terminator that
  // ended the sequence, and a reply has to echo the one the query used: a
  // program reading until the terminator it sent would otherwise wait for one
  // that never comes. The bytes are scanned on the way in and each terminator
  // queued for the handler that is about to run.
  //
  // Each entry is the code of an OSC sequence and whether it ended in `BEL`.
  // Keyed by code because the scan sees every sequence while only the color
  // ones are answered here: a title arriving between two queries would
  // otherwise shift the queue out of step and put the wrong terminator on a
  // reply.
  //
  // The scan carries its state between calls because a PTY read splits
  // wherever it likes, including between the two bytes of an `ST`.
  //
  // Only the codes answered here are recorded. The scan sees every sequence,
  // and one whose terminator is never claimed would sit in this list for the
  // life of the session: a shell that retitles the window on every prompt
  // would leak an entry per prompt.
  var ANSWERED = { 4: 1, 10: 1, 11: 1, 12: 1, 104: 1, 110: 1, 111: 1, 112: 1 };
  var terminators = [];
  var inOsc = false, sawEsc = false, code = -1, readingCode = false;
  function scanTerminators(bytes) {
    for (var i = 0; i < bytes.length; i++) {
      var b = bytes[i];
      if (inOsc) {
        // `BEL` ends the sequence, and so does a bare `ESC`: the parser these
        // bytes are about to reach ends an OSC the moment it sees one, without
        // waiting to learn whether the `\` of an `ST` follows. Recording the
        // terminator on that `ESC` rather than on the `\` is what keeps this
        // scan in step with the handler when a read splits between the two.
        if (b === 0x07 || b === 0x1b) {
          if (ANSWERED[code]) { terminators.push({ code: code, bel: b === 0x07 }); }
          inOsc = false; readingCode = false; code = -1; sawEsc = false;
          continue;
        }
        if (readingCode) {
          if (b >= 0x30 && b <= 0x39) { code = (code < 0 ? 0 : code) * 10 + (b - 0x30); }
          else { readingCode = false; }
        }
        continue;
      }
      if (sawEsc) {
        sawEsc = false;
        if (b === 0x5d) { inOsc = true; readingCode = true; code = -1; continue; }
      }
      if (b === 0x1b) { sawEsc = true; }
    }
  }

  // Whether the sequence being handled ended in `BEL`. Defaults to `BEL`,
  // which is what a query whose start this scan never saw is most likely to
  // have used.
  function tookBel(wanted) {
    for (var i = 0; i < terminators.length; i++) {
      if (terminators[i].code === wanted) { return terminators.splice(i, 1)[0].bel; }
    }
    return true;
  }

  // `parseInt` reads the leading digits of `1x` and calls it slot 1, which
  // addresses a slot the sequence never named. A malformed index is not an
  // index, so it names nothing.
  function slotIndex(text) {
    return /^[0-9]+$/.test(text) ? parseInt(text, 10) : -1;
  }

  // `rgb:RRRR/GGGG/BBBB`, the form xterm replies in: each component doubled
  // to sixteen bits, which is what programs parse.
  function spec(v) {
    function pair(c) { var h = (c & 0xff).toString(16); if (h.length < 2) { h = '0' + h; } return h + h; }
    return 'rgb:' + pair(v >> 16) + '/' + pair(v >> 8) + '/' + pair(v);
  }

  // A reply carries the terminator the query used: a program that reads until
  // the one it sent would otherwise wait for a terminator that never comes.
  function answer(prefix, value, bel) {
    replies.push('\x1b]' + prefix + ';' + spec(value) + (bel ? '\x07' : '\x1b\\'));
  }

  // `#rrggbb`, `#rgb`, and `rgb:r/g/b` with one to four hex digits each, which
  // is the range xterm accepts.
  function parseColor(text) {
    var m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(text);
    if (m) {
      var h = m[1];
      if (h.length === 3) { h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2]; }
      return parseInt(h, 16);
    }
    m = /^rgb:([0-9a-f]{1,4})\/([0-9a-f]{1,4})\/([0-9a-f]{1,4})$/i.exec(text);
    if (!m) { return null; }
    var out = 0;
    for (var i = 1; i <= 3; i++) {
      var part = m[i];
      // Scale whatever width was given down to eight bits, the way xterm
      // does: `f` is full intensity just as `ffff` is.
      var max = Math.pow(16, part.length) - 1;
      out = (out << 8) | Math.round((parseInt(part, 16) / max) * 255);
    }
    return out;
  }

  // Setting and querying a dynamic color share one sequence, told apart by a
  // payload of `?`. Returning false leaves the sequence unhandled so xterm.js
  // still sees it, which matters for the ones it acts on itself.
  function dynamic(prefix, slot) {
    term.parser.registerOscHandler(prefix, function (data) {
      var parts = String(data).split(';');
      var bel = tookBel(prefix);
      for (var i = 0; i < parts.length; i++) {
        if (parts[i] === '?') {
          answer(prefix, resolved(slot), bel);
        } else {
          var v = parseColor(parts[i]);
          if (v !== null) { overrides[slot] = v; }
        }
      }
      return true;
    });
  }
  dynamic(10, FG);
  dynamic(11, BG);
  dynamic(12, CURSOR);

  // `OSC 4` addresses the palette, so each pair names its own slot.
  term.parser.registerOscHandler(4, function (data) {
    var parts = String(data).split(';');
    var bel = tookBel(4);
    for (var i = 0; i + 1 < parts.length; i += 2) {
      var slot = slotIndex(parts[i]);
      if (!(slot >= 0 && slot <= 255)) { continue; }
      if (parts[i + 1] === '?') {
        replies.push('\x1b]4;' + slot + ';' + spec(resolved(slot)) + (bel ? '\x07' : '\x1b\\'));
      } else {
        var v = parseColor(parts[i + 1]);
        if (v !== null) { overrides[slot] = v; }
      }
    }
    return true;
  });

  // Resets drop the runtime value so the configured one shows through again.
  term.parser.registerOscHandler(104, function (data) {
    tookBel(104);
    var text = String(data);
    if (text === '') {
      for (var k = 0; k <= 255; k++) { delete overrides[k]; }
      return true;
    }
    var parts = text.split(';');
    for (var i = 0; i < parts.length; i++) {
      var slot = slotIndex(parts[i]);
      if (slot >= 0 && slot <= 255) { delete overrides[slot]; }
    }
    return true;
  });
  [[110, FG], [111, BG], [112, CURSOR]].forEach(function (pair) {
    term.parser.registerOscHandler(pair[0], function () { tookBel(pair[0]); delete overrides[pair[1]]; return true; });
  });

  function drain() {
    // `_timers` grows while draining, so re-check rather than snapshotting.
    // The cap turns a hypothetical self-rescheduling timer into an error
    // instead of a hung reader thread.
    var guard = 0;
    while (__timers.length) {
      __timers.shift()();
      if (++guard > 1000000) { throw new Error('xterm.js timer queue did not settle'); }
    }
  }

  // One reused cell object across the whole grid walk. `getCell(x, cell)`
  // fills it in place; the allocating form costs roughly twice as much.
  var CELL = term.buffer.active.getNullCell();

  return {
    feed: function (bytes) { scanTerminators(bytes); term.write(bytes); drain(); },

    // Joined rather than returned as an array: one string crossing the
    // boundary beats one call per pending reply.
    takeReplies: function () { var s = replies.join(''); replies.length = 0; return s; },

    resize: function (cols, rows) { term.resize(cols, rows); drain(); },
    cols: function () { return term.cols; },
    rows: function () { return term.rows; },
    cursorX: function () { return term.buffer.active.cursorX; },
    cursorY: function () { return term.buffer.active.cursorY; },

    title: function () { return title; },

    // `DECTCEM`. xterm.js spells a hidden cursor as a flag on the core
    // service rather than as a mode this shim can read back.
    cursorVisible: function () { return !term._core.coreService.isCursorHidden; },

    // `DECSCUSR`. The property is absent until a program sets one, and a
    // terminal that has not been told otherwise draws a block.
    cursorShape: function () {
      var s = term._core.coreService.decPrivateModes.cursorStyle;
      return s === undefined ? 'block' : String(s);
    },

    // A color a program set, or -1 when it has not touched this slot and the
    // profile still decides. Kept as one call per slot: only a handful are
    // ever read, and a whole-table crossing would cost more than it saves.
    colorOverride: function (slot) {
      var v = overrides[slot];
      return v === undefined ? -1 : v;
    },

    // Row span of the visible screen; `full` prepends the scrollback.
    start: function (full) { return full ? 0 : term.buffer.active.baseY; },
    end: function (full) {
      var b = term.buffer.active;
      return full ? b.length : b.baseY + term.rows;
    },

    // The grid crosses the boundary as exactly two values: every cell's text
    // in one NUL-joined string, and six ints per cell in one flat array. NUL
    // is safe as a separator because xterm.js reports an empty string, never
    // a NUL, for a cell holding nothing.
    //
    // Per-cell ints are `[width, fg, bg, ulColor, ulStyle, flags]`, with the
    // color *modes* packed into `flags` alongside the SGR booleans: a raw
    // color of 1 is palette slot 1 or the RGB triple #000001 depending on its
    // mode, so the mode has to travel with it.
    pack: function (start, end) {
      var buf = term.buffer.active, cols = term.cols;
      var chars = [], meta = [];
      for (var y = start; y < end; y++) {
        var line = buf.getLine(y);
        for (var x = 0; x < cols; x++) {
          if (!line) { chars.push(' '); meta.push(1, -1, -1, -1, 0, 0); continue; }
          var c = line.getCell(x, CELL);
          chars.push(c.getChars());

          // Reading a cell costs a JS call per getter, and a full-scrollback
          // dump is hundreds of thousands of cells. Most of them are ordinary
          // unstyled text, and for those one call answers all nineteen: -1 is
          // the "no color" value every getter returns, and no attribute bit is
          // set. Verified to produce byte-identical output to the long form
          // across every SGR in the cell vocabulary.
          if (c.isAttributeDefault()) { meta.push(c.getWidth(), -1, -1, -1, 0, 0); continue; }

          var fg = c.getFgColor();
          var fgMode = c.isFgPalette() ? 1 : (c.isFgRGB() ? 2 : 0);
          var ulColor = c.getUnderlineColor();
          var ulMode = c.isUnderlineColorPalette() ? 1 : (c.isUnderlineColorRGB() ? 2 : 0);

          // xterm.js keeps the underline color in an extended-attribute
          // record that it drops whenever the underline style is NONE, and
          // both underline-color getters then fall back to reporting the
          // foreground. Left alone that shows up as every colored cell
          // claiming an underline color it was never given. Collapsing the
          // case where the two are identical back to "unset" is exactly the
          // vocabulary's own spelling for it: `underline_color: None` already
          // means the underline takes the foreground. A cell that really did
          // set SGR 58 to its own foreground color lands here too, and draws
          // the same either way.
          if (ulColor === fg && ulMode === fgMode) { ulMode = 0; }

          // SGR 59 (reset underline color) does not clear the record: it
          // stores a sentinel that reads back through the public getters as
          // RGB #ffffff, so an ordinary reset produced a white underline where
          // there should be none. The sentinel is indistinguishable from a
          // real `58;2;255;255;255` at this layer -- both report RGB with
          // value 0xffffff -- so one of the two has to be wrong. Resetting is
          // overwhelmingly the more common of the two, and getting it wrong
          // paints a color the terminal never asked for, so it wins.
          if (ulMode === 2 && ulColor === 0xffffff) { ulMode = 0; }

          var flags =
            (c.isBold() ? 1 : 0) |
            (c.isDim() ? 2 : 0) |
            (c.isItalic() ? 4 : 0) |
            (c.isInverse() ? 8 : 0) |
            (c.isInvisible() ? 16 : 0) |
            (c.isStrikethrough() ? 32 : 0) |
            (c.isBlink() ? 64 : 0) |
            (fgMode === 1 ? 256 : (fgMode === 2 ? 512 : 0)) |
            (c.isBgPalette() ? 1024 : (c.isBgRGB() ? 2048 : 0)) |
            (ulMode === 1 ? 4096 : (ulMode === 2 ? 8192 : 0));

          meta.push(c.getWidth(), fg, c.getBgColor(), ulColor, c.getUnderlineStyle(), flags);
        }
      }
      return [chars.join('\0'), meta];
    },
  };
};
