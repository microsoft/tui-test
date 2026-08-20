# Vendored xterm.js assets

Compiled into the `tui-test` binary by `crates/tui-test/src/terminal/xtermjs.rs`
so the xterm.js backend needs no Node.js at runtime.

| File | Source | Version | License |
| ---- | ------ | ------- | ------- |
| `xterm-headless.js` | [`@xterm/headless`](https://www.npmjs.com/package/@xterm/headless) | 6.0.0 | MIT |
| `addon-unicode11.js` | [`@xterm/addon-unicode11`](https://www.npmjs.com/package/@xterm/addon-unicode11) | 0.9.0 | MIT |
| `LICENSE` | both of the above, reproduced per package | — | MIT |

`shim.js` is tui-test's own code, not vendored.

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

## Updating

Re-fetch both packages at the versions in the table and copy the bundles in
unchanged, which also re-checks the hashes above against what npm serves today:

```sh
npm pack @xterm/headless@6.0.0 @xterm/addon-unicode11@0.9.0
tar xzOf xterm-headless-6.0.0.tgz package/lib-headless/xterm-headless.js > xterm-headless.js
tar xzOf xterm-addon-unicode11-0.9.0.tgz package/lib/addon-unicode11.js > addon-unicode11.js
shasum -a 256 xterm-headless.js addon-unicode11.js
```

Then run `cargo test -p tui-test-rs --features xtermjs conformance`, which
checks the backend against the same contract every other one meets, and
re-measure the table above before changing a version. A version bump also means
re-checking each package's `LICENSE`, since they carry separate notices.
