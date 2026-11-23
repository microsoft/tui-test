#!/usr/bin/env bash
# Shared toolchain setup for native engines and Python wheels.
set -euo pipefail

curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs |
  sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path

version=0.15.2
case "$(uname -m)" in
  x86_64|amd64)
    arch=x86_64
    sha256=02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239
    ;;
  aarch64|arm64)
    arch=aarch64
    sha256=958ed7d1e00d0ea76590d27666efbf7a932281b3d7ba0c6b01b0ff26498f667f
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
