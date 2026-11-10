#!/usr/bin/env bash
# Shared toolchain setup for native engines and Python wheels.
set -euo pipefail

curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs |
  sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path

version=0.17.0
case "$(uname -m)" in
  x86_64|amd64)
    arch=x86_64
    sha256=1cbe9df9f27e6b78d14ccbca43b6703a404ef79ef1c463de901d7f088d4e2026
    ;;
  aarch64|arm64)
    arch=aarch64
    sha256=9e8d11661d4ae3bd57702a3832781e23ad151dde5798e16a5ccd503f65234ff8
    ;;
  *)
    echo "unsupported Zig host architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

mkdir -p "$HOME/.local/bin" "$HOME/.local/lib" "$HOME/.cache/tui-test-release"
archive="$HOME/.cache/tui-test-release/zig-$arch-linux-$version.tar.xz"
curl --proto '=https' --tlsv1.2 -fsSLo "$archive" \
  "https://ziglang.org/download/$version/zig-$arch-linux-$version.tar.xz"
echo "$sha256  $archive" | sha256sum -c -
tar -xJf "$archive" -C "$HOME/.local/lib"
ln -sf "$HOME/.local/lib/zig-$arch-linux-$version/zig" "$HOME/.local/bin/zig"
"$HOME/.local/bin/zig" version
