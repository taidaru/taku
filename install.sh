#!/bin/sh
# Quick installer for taku on Unix systems (Linux, macOS).
#
#   curl -fsSL https://raw.githubusercontent.com/taidaru/taku/main/install.sh | sh
#
# Downloads the latest release binary for the detected OS/arch and installs it.
#
# Environment overrides:
#   TAKU_INSTALL_DIR   target directory (default: ~/.local/bin, or /usr/local/bin if writable & in PATH)
#   TAKU_VERSION       release tag to install (default: latest)

set -eu

REPO="taidaru/taku"
BIN="taku"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }
info() { printf '%s\n' "$1" >&2; }

# --- detect platform -> release target triple --------------------------------
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux)  os_part="unknown-linux-gnu" ;;
  Darwin) os_part="apple-darwin" ;;
  *) err "unsupported OS '$os'. On Windows, install via Scoop" ;;
esac

case "$arch" in
  x86_64|amd64) arch_part="x86_64" ;;
  arm64|aarch64) arch_part="aarch64" ;;
  *) err "unsupported architecture '$arch'" ;;
esac

target="${arch_part}-${os_part}"
asset="${BIN}-${target}.tar.gz"

# --- resolve download URL -----------------------------------------------------
version="${TAKU_VERSION:-latest}"
if [ "$version" = "latest" ]; then
  url="https://github.com/$REPO/releases/latest/download/$asset"
else
  url="https://github.com/$REPO/releases/download/$version/$asset"
fi

# --- fetcher ------------------------------------------------------------------
if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fSL --proto '=https' --tlsv1.2 "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -O "$2" "$1"; }
else
  err "need curl or wget to download"
fi

# --- choose install dir -------------------------------------------------------
in_path() {
  case ":$PATH:" in *":$1:"*) return 0 ;; *) return 1 ;; esac
}

if [ -n "${TAKU_INSTALL_DIR:-}" ]; then
  install_dir="$TAKU_INSTALL_DIR"
elif [ -w /usr/local/bin ] && in_path /usr/local/bin; then
  install_dir="/usr/local/bin"
else
  install_dir="$HOME/.local/bin"
fi

# --- download, extract, install ----------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

info "downloading $asset ($version)..."
fetch "$url" "$tmp/$asset" || err "download failed: $url"

tar -xzf "$tmp/$asset" -C "$tmp" || err "failed to extract $asset"
[ -f "$tmp/$BIN" ] || err "archive did not contain '$BIN'"

mkdir -p "$install_dir"
chmod +x "$tmp/$BIN"
mv -f "$tmp/$BIN" "$install_dir/$BIN"

info "installed $BIN to $install_dir/$BIN"

if ! in_path "$install_dir"; then
  info ""
  info "note: $install_dir is not on your PATH. Add it, e.g.:"
  info "  export PATH=\"$install_dir:\$PATH\""
fi
