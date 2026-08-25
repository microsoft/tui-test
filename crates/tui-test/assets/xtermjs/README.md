# Vendored xterm.js assets

Compiled into the `tui-test` binary by `crates/tui-test/src/terminal/xtermjs.rs`
so the xterm.js backend needs no Node.js at runtime.

| File | Source | License |
| ---- | ------ | ------- |
| `xterm-headless.js` | [`@xterm/headless`](https://www.npmjs.com/package/@xterm/headless) | MIT |
| `addon-unicode11.js` | [`@xterm/addon-unicode11`](https://www.npmjs.com/package/@xterm/addon-unicode11) | MIT |
| `LICENSE` | both of the above, reproduced per package | MIT |

`shim.js` is tui-test's own code, not vendored.

Both bundles are dropped in unchanged and committed, so building tui-test needs
neither Node nor a network. `pinned.json` records the versions they came from.
Updating them is manual — see below — and nothing in CI rewrites them.

## Why the unicode11 addon

The headless bundle ships only the Unicode 6 width tables, which measure astral
emoji as one column. alacritty measures them as two, so without this a line
containing an emoji reports every following cell in a different column on the
two backends.

Newer is not better here. Measured cursor column after each sequence:

| Input | alacritty | v6 | **v11** | v15 | v15-graphemes |
| ----- | --------- | -- | ------- | --- | ------------- |
| `🙂X` | 3 | 2 | **3** | 3 | 3 |
| `👨‍👩X` | 5 | 3 | **5** | 6 | 3 |
| `👍🏽X` | 5 | 3 | **5** | 5 | 3 |
| `🇺🇸X` | 3 | 3 | **3** | 3 | 3 |
| `你X` | 3 | 3 | **3** | 3 | 3 |

Only v11 agrees with alacritty on every case, which is why it is pinned there.

## What the shim adds

`@xterm/headless` leaves out a few things the [`Emulator`] contract asks for,
and the shim supplies them rather than the Rust side working around them:

| Contract | How |
| -------- | --- |
| Window title | `onTitleChange`, with `windowOptions.pushTitle`/`popTitle` enabling the `CSI 22/23 t` stack the bundle implements but leaves off |
| Cursor visibility | `coreService.isCursorHidden` |
| Cursor shape | `coreService.decPrivateModes.cursorStyle`, absent until `DECSCUSR` sets one, so it reads as a block until then |
| `OSC 4/10/11/12` set, query, and reset | `parser.registerOscHandler`, answering out of the same reply queue the terminal's own replies use so answers keep the order they were asked in |
| `SGR 59` | xterm.js stores the reset as an explicit white, indistinguishable from a real `58;2;255;255;255` once written; the shim clears it where the parser sets it, while the two are still apart |

A query has to echo the terminator it was asked with, and an OSC handler is
given its payload but not that terminator, so the shim scans the incoming bytes
for it. The scan carries state between calls because a PTY read splits wherever
it likes, and it keys what it finds by OSC code so that a title arriving between
two colour queries cannot put the wrong terminator on a reply.

## Known divergence

xterm.js records a cell's underline colour only when that cell also has an
underline style, so `SGR 58` on its own is not readable back off the cell.
Nothing renders differently. The backend declares this where it opts into the
conformance suite, so the exception is visible rather than silent.

Two addons are deliberately not used yet: [#175](https://github.com/microsoft/tui-test/issues/175).

## Updating

```sh
.github/scripts/vendor-xtermjs.sh --latest   # or omit --latest to re-fetch the pinned versions
```

Then run `cargo test -p tui-test-rs --features xtermjs conformance`, re-measure
the width table above, and re-check each package's `LICENSE`, since they carry
separate notices. The release workflow fails if these are not on the latest
published versions.
