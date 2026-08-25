#!/usr/bin/env bash
# Fetch the xterm.js bundles into crates/tui-test/assets/xtermjs.
#
# The bundles are not checked into the repository. `crates/tui-test/build.rs`
# fetches them into OUT_DIR when they are missing, which covers ordinary
# builds; this script puts them in the source tree instead, which is what
# `cargo package` needs in order to carry them in the published crate.
set -euo pipefail

assets="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/crates/tui-test/assets/xtermjs"
pinned="$assets/pinned.json"

headless="$(jq -r '."@xterm/headless"' "$pinned")"
unicode11="$(jq -r '."@xterm/addon-unicode11"' "$pinned")"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"

npm pack "@xterm/headless@$headless" "@xterm/addon-unicode11@$unicode11" >/dev/null
tar xzOf "xterm-headless-$headless.tgz" package/lib-headless/xterm-headless.js \
  > "$assets/xterm-headless.js"
tar xzOf "xterm-addon-unicode11-$unicode11.tgz" package/lib/addon-unicode11.js \
  > "$assets/addon-unicode11.js"

echo "Vendored @xterm/headless@$headless and @xterm/addon-unicode11@$unicode11 into $assets."
