#!/usr/bin/env bash
# Shared toolchain setup for native engines and Python wheels.
set -euo pipefail

curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs |
  sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path

version=0.16.0
case "$(uname -m)" in
  x86_64|amd64)
    arch=x86_64
    sha256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
    ;;
  aarch64|arm64)
    arch=aarch64
    sha256=ea4b09bfb22ec6f6c6ceac57ab63efb6b46e17ab08d21f69f3a48b38e1534f17
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
