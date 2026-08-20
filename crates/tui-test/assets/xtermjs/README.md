# Vendored xterm.js assets

Compiled into the `tui-test` binary by `crates/tui-test/src/terminal/xtermjs.rs`
so the xterm.js backend needs no Node.js at runtime.

| File | Source | License |
| ---- | ------ | ------- |
| `xterm-headless.js` | [`@xterm/headless`](https://www.npmjs.com/package/@xterm/headless) | MIT |
| `addon-unicode11.js` | [`@xterm/addon-unicode11`](https://www.npmjs.com/package/@xterm/addon-unicode11) | MIT |
| `LICENSE` | both of the above, reproduced per package | MIT |

`shim.js` is tui-test's own code, not vendored.

`pinned.json` holds the versions these bundles came from, and is the only place
they are written down so that nothing can claim a version the bytes are not.
Both bundles are dropped in unchanged, so they are byte-for-byte what npm
publishes:

```
17a90b650cf6b77cce2b98c4063884d43545e4ce177a54b76ccfc906f1aacaed  xterm-headless.js
72353b5178e1a7382716df1cfedf8ab070eea655d38995bb9f4f284fe56e2f2b  addon-unicode11.js
```

## Why the unicode11 addon

The headless bundle ships only the Unicode 6 width tables, which measure astral
emoji as one column. alacritty measures them as two, so without this a line
containing an emoji reports every following cell in a different column on the
two backends. Unicode 11 restores the pair.

Newer is not better here. Measured cursor column after each sequence:

| Input | alacritty | v6 | **v11** | v15 | v15-graphemes |
| ----- | --------- | -- | ------- | --- | ------------- |
| `🙂X` | 3 | 2 | **3** | 3 | 3 |
| `👨‍👩X` | 5 | 3 | **5** | 6 | 3 |
| `👍🏽X` | 5 | 3 | **5** | 5 | 3 |
| `🇺🇸X` | 3 | 3 | **3** | 3 | 3 |
| `你X` | 3 | 3 | **3** | 3 | 3 |

Only v11 agrees with alacritty on every case. `@xterm/addon-unicode-graphemes`
(v15 / v15-graphemes) is also marked experimental by its own package
description, and needs an `atob` the QuickJS host does not have.

## What the shim adds

`@xterm/headless` leaves out a few things the [`Emulator`] contract asks for,
and the shim supplies them rather than the Rust side working around them:

| Contract | How |
| -------- | --- |
| Window title | `onTitleChange`, with `windowOptions.pushTitle`/`popTitle` enabling the `CSI 22/23 t` stack the bundle implements but leaves off |
| Cursor visibility | `coreService.isCursorHidden` |
| Cursor shape | `coreService.decPrivateModes.cursorStyle`, absent until `DECSCUSR` sets one, so it reads as a block until then |
| `OSC 4/10/11/12` set, query, and reset | `parser.registerOscHandler`, answering out of the same reply queue the terminal's own replies use so answers keep the order they were asked in |

A query has to echo the terminator it was asked with, and an OSC handler is
given its payload but not that terminator, so the shim scans the incoming bytes
for it. The scan carries state between calls because a PTY read splits wherever
it likes, and it keys what it finds by OSC code so that a title arriving between
two colour queries cannot put the wrong terminator on a reply. It records the
terminator on the `ESC` of an `ST` rather than on the `\` that follows, because
the parser ends an OSC as soon as it sees that `ESC` without waiting to learn
what comes next, and only for the codes answered here, so that a sequence whose
terminator nothing claims cannot accumulate.

## Known divergence

xterm.js records a cell's underline colour only when that cell also has an
underline style: `SGR 58` on its own, or a colour that outlives an `SGR 24`, is
not readable back off the cell. Nothing renders differently, since a cell with
no underline draws no underline colour either way. The backend declares this
where it opts into the conformance suite, so the exception is visible rather
than silent.

## Addons we do not use yet

Two are worth revisiting, neither of them yet:

`@xterm/addon-unicode-graphemes` would replace the unicode11 addon, but it is
marked experimental by its own package description, needs an `atob` the QuickJS
host does not have, and measures two of the cases in the table above
differently from alacritty. Worth re-measuring against that table once it is no
longer experimental, and only adopting if it still agrees.

`@xterm/addon-clipboard` implements `OSC 52`, which tui-test does not support
on any backend today: alacritty parses it and raises `ClipboardStore` and
`ClipboardLoad`, and the listener in `terminal/alacritty.rs` drops both. Adding
it here first would make things worse rather than better — `OSC 52;c;?` is
answered by nothing today, and this addon would make xterm.js alone answer it,
so the same sequence would behave differently depending on the backend. `OSC
52` wants to land in the [`Emulator`] contract and the conformance suite first;
then this addon is how xterm.js implements its half.

## Updating

`.github/workflows/xtermjs-update.yml` does this weekly and opens a pull
request. To do it by hand, from this directory:

```sh
headless="$(jq -r '."@xterm/headless"' pinned.json)"
unicode11="$(jq -r '."@xterm/addon-unicode11"' pinned.json)"
npm pack "@xterm/headless@$headless" "@xterm/addon-unicode11@$unicode11"
tar xzOf "xterm-headless-$headless.tgz" package/lib-headless/xterm-headless.js > xterm-headless.js
tar xzOf "xterm-addon-unicode11-$unicode11.tgz" package/lib/addon-unicode11.js > addon-unicode11.js
shasum -a 256 xterm-headless.js addon-unicode11.js
```

Then run `cargo test -p tui-test-rs --features xtermjs conformance`, which
checks the backend against the same contract every other one meets, and
re-measure the width table above before changing a version. A version bump also
means re-checking each package's `LICENSE`, since they carry separate notices.
`.github/workflows/vendored.yml` re-fetches both packages whenever these files
change and fails if what is checked in is not byte for byte what npm publishes.
