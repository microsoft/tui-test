#!/usr/bin/env bash
# Re-vendor the xterm.js bundles in crates/tui-test/assets/xtermjs.
#
#   vendor-xtermjs.sh            re-fetch the versions pinned.json already names
#   vendor-xtermjs.sh --latest   bump pinned.json to the latest releases first
#
# The bundles are checked into the repository so that building tui-test needs
# neither Node nor a network. This script is how they are updated.
set -euo pipefail

assets="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/crates/tui-test/assets/xtermjs"
pinned="$assets/pinned.json"

if [ "${1:-}" = "--latest" ]; then
  headless="$(npm view @xterm/headless version)"
  unicode11="$(npm view @xterm/addon-unicode11 version)"
  jq -n --arg h "$headless" --arg u "$unicode11" \
    '{"@xterm/headless": $h, "@xterm/addon-unicode11": $u}' > "$pinned"
else
  headless="$(jq -r '."@xterm/headless"' "$pinned")"
  unicode11="$(jq -r '."@xterm/addon-unicode11"' "$pinned")"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
(
  cd "$work"
  npm pack "@xterm/headless@$headless" "@xterm/addon-unicode11@$unicode11" >/dev/null 2>&1
  tar xzOf "xterm-headless-$headless.tgz" package/lib-headless/xterm-headless.js \
    > "$assets/xterm-headless.js"
  tar xzOf "xterm-addon-unicode11-$unicode11.tgz" package/lib/addon-unicode11.js \
    > "$assets/addon-unicode11.js"
)

echo "Vendored @xterm/headless@$headless and @xterm/addon-unicode11@$unicode11."
echo
echo "Next:"
echo "  cargo test -p tui-test-rs --features xtermjs"
echo "  re-measure the emoji width table in $assets/README.md"
echo "  re-check each package's LICENSE, which carry separate notices"
