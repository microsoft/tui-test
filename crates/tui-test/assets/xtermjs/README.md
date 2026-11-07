# Vendored xterm.js assets

`xterm-headless.js` and `addon-unicode11.js` are unchanged bundles from
`@xterm/headless` and `@xterm/addon-unicode11`, both MIT licensed.
`LICENSE` contains their notices; `pinned.json` records their versions.
`shim.js` is tui-test's code.

The bundles are compiled into the binary, so the xterm.js backend needs no
Node.js at runtime.

See the [xterm.js backend reference](https://tui-test.dev/docs/reference/xtermjs/)
for Unicode width comparisons, shim behavior, known differences, and updates.
