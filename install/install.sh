#!/bin/sh
set -eu

REPOSITORY="microsoft/tui-test"
BINARY_NAME="tui-test"

fail() {
  echo "Error: $*" >&2
  exit 1
}

command -v uname >/dev/null 2>&1 || fail "uname is required."
command -v tar >/dev/null 2>&1 || fail "tar is required."

case "$(uname -s)" in
  Darwin) OS="apple-darwin" ;;
  Linux) OS="unknown-linux-musl" ;;
  MINGW*|MSYS*|CYGWIN*)
    fail "Use install.ps1 to install tui-test on Windows."
    ;;
  *) fail "Unsupported operating system: $(uname -s)" ;;
esac

case "$(uname -m)" in
  x86_64|amd64) ARCH="x86_64" ;;
  arm64|aarch64) ARCH="aarch64" ;;
  *) fail "Unsupported architecture: $(uname -m)" ;;
esac

TARGET="${ARCH}-${OS}"
ASSET="${BINARY_NAME}-${TARGET}.tar.gz"
VERSION="${TUI_TEST_VERSION:-latest}"
TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t tui-test)"
ARCHIVE_PATH="${TMP_DIR}/${ASSET}"
EXTRACT_DIR="${TMP_DIR}/extract"

cleanup() {
  rm -rf -- "$TMP_DIR"
}
trap cleanup EXIT HUP INT TERM

download() {
  url="$1"
  destination="$2"
  use_token="${3:-true}"

  if command -v curl >/dev/null 2>&1; then
    if [ "$use_token" = "true" ] && [ -n "$TOKEN" ]; then
      curl --proto '=https' --tlsv1.2 -fsSL \
        -H "Authorization: Bearer ${TOKEN}" \
        "$url" -o "$destination"
    else
      curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$destination"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if [ "$use_token" = "true" ] && [ -n "$TOKEN" ]; then
      wget -q --header="Authorization: Bearer ${TOKEN}" \
        -O "$destination" "$url"
    else
      wget -q -O "$destination" "$url"
    fi
  else
    fail "curl or wget is required."
  fi
}

if [ -z "$VERSION" ] || [ "$VERSION" = "latest" ]; then
  RELEASE_URL="https://github.com/${REPOSITORY}/releases/latest/download"
elif [ "$VERSION" = "beta" ]; then
  command -v awk >/dev/null 2>&1 || fail "awk is required to resolve beta releases."
  RELEASES_PATH="${TMP_DIR}/releases.json"
  download \
    "https://api.github.com/repos/${REPOSITORY}/releases?per_page=100" \
    "$RELEASES_PATH" \
    false ||
    fail "Could not query GitHub releases."

  VERSION="$(
    awk '
      /^    "tag_name": "/ {
        tag = $0
        sub(/^    "tag_name": "/, "", tag)
        sub(/",?$/, "", tag)
        draft = ""
      }
      /^    "draft": false,?$/ { draft = "false" }
      /^    "draft": true,?$/ { draft = "true" }
      /^    "prerelease": true,?$/ &&
        draft == "false" &&
        tag ~ /^[0-9]+\.[0-9]+\.[0-9]+-beta\.[0-9]+$/ {
        print tag
        exit
      }
    ' "$RELEASES_PATH"
  )"
  [ -n "$VERSION" ] || fail "No beta release was found."
  RELEASE_URL="https://github.com/${REPOSITORY}/releases/download/${VERSION}"
else
  RELEASE_URL="https://github.com/${REPOSITORY}/releases/download/${VERSION}"
fi

DOWNLOAD_URL="${RELEASE_URL}/${ASSET}"

echo "Downloading tui-test for ${TARGET}..."
download "$DOWNLOAD_URL" "$ARCHIVE_PATH" ||
  fail "Could not download ${DOWNLOAD_URL}"

ARCHIVE_CONTENTS="$(tar -tzf "$ARCHIVE_PATH")" ||
  fail "Downloaded archive is invalid."
case "$ARCHIVE_CONTENTS" in
  "$BINARY_NAME"|"./$BINARY_NAME") ;;
  *) fail "Downloaded archive has unexpected contents." ;;
esac

mkdir -p "$EXTRACT_DIR"
tar -xzf "$ARCHIVE_PATH" -C "$EXTRACT_DIR"

if [ -n "${TUI_TEST_INSTALL_DIR:-}" ]; then
  INSTALL_DIR="$TUI_TEST_INSTALL_DIR"
elif [ -n "${PREFIX:-}" ]; then
  INSTALL_DIR="${PREFIX}/bin"
elif [ "$(id -u 2>/dev/null || echo 1)" -eq 0 ]; then
  INSTALL_DIR="/usr/local/bin"
else
  : "${HOME:?HOME is required when installing without root privileges.}"
  INSTALL_DIR="${HOME}/.local/bin"
fi

mkdir -p "$INSTALL_DIR" ||
  fail "Could not create ${INSTALL_DIR}. Set TUI_TEST_INSTALL_DIR to a writable directory."

DESTINATION="${INSTALL_DIR}/${BINARY_NAME}"
STAGED_DESTINATION="${INSTALL_DIR}/.${BINARY_NAME}.tmp.$$"
cp "${EXTRACT_DIR}/${BINARY_NAME}" "$STAGED_DESTINATION"
chmod 755 "$STAGED_DESTINATION"
mv -f "$STAGED_DESTINATION" "$DESTINATION"

echo "Installed tui-test to ${DESTINATION}"

case ":${PATH:-}:" in
  *":${INSTALL_DIR}:"*) exit 0 ;;
esac

CURRENT_SHELL="$(basename "${SHELL:-/bin/sh}")"
case "$CURRENT_SHELL" in
  zsh)
    RC_FILE="${ZDOTDIR:-$HOME}/.zprofile"
    PATH_LINE="export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
  bash)
    if [ -f "$HOME/.bash_profile" ]; then
      RC_FILE="$HOME/.bash_profile"
    elif [ -f "$HOME/.bash_login" ]; then
      RC_FILE="$HOME/.bash_login"
    else
      RC_FILE="$HOME/.profile"
    fi
    PATH_LINE="export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
  fish)
    RC_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/tui-test.fish"
    PATH_LINE="fish_add_path \"${INSTALL_DIR}\""
    ;;
  *)
    RC_FILE="$HOME/.profile"
    PATH_LINE="export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

case "${TUI_TEST_NO_MODIFY_PATH:-}" in
  1|true|TRUE|yes|YES)
    echo "Add ${INSTALL_DIR} to PATH."
    exit 0
    ;;
esac

mkdir -p "$(dirname "$RC_FILE")"
if ! grep -Fqx "$PATH_LINE" "$RC_FILE" 2>/dev/null; then
  printf "\n%s\n" "$PATH_LINE" >>"$RC_FILE"
  echo "Added ${INSTALL_DIR} to PATH. Restart your shell."
fi
