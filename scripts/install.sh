#!/bin/sh
set -eu

REPO="ftorrresd/glissues"
BIN_NAME="glissues"
INSTALL_DIR="${GLISSUES_INSTALL_DIR:-$HOME/.local/bin}"
API_URL="https://api.github.com/repos/$REPO/releases/latest"

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'glissues installer: missing required command: %s\n' "$1" >&2
    exit 1
  }
}

need_cmd curl
need_cmd tar
need_cmd mktemp

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)

case "$os" in
  linux)
    case "$arch" in
      x86_64|amd64) target="x86_64-unknown-linux-gnu" ;;
      *)
        printf 'glissues installer: unsupported Linux architecture: %s\n' "$arch" >&2
        exit 1
        ;;
    esac
    ;;
  darwin)
    case "$arch" in
      x86_64) target="x86_64-apple-darwin" ;;
      arm64|aarch64) target="aarch64-apple-darwin" ;;
      *)
        printf 'glissues installer: unsupported macOS architecture: %s\n' "$arch" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    printf 'glissues installer: unsupported operating system: %s\n' "$os" >&2
    exit 1
    ;;
esac

asset="glissues-$target.tar.gz"
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT INT TERM

printf 'glissues installer: resolving latest release for %s\n' "$asset"
download_url=$(curl -fsSL "$API_URL" | sed -n "s/.*\"browser_download_url\": \"\([^\"]*${asset}\)\".*/\1/p" | head -n 1)

if [ -z "$download_url" ]; then
  printf 'glissues installer: could not find asset %s in latest release\n' "$asset" >&2
  exit 1
fi

archive="$tmpdir/$asset"
printf 'glissues installer: downloading %s\n' "$download_url"
curl -fL "$download_url" -o "$archive"

mkdir -p "$INSTALL_DIR"
tar -xzf "$archive" -C "$tmpdir"
install_path="$INSTALL_DIR/$BIN_NAME"
cp "$tmpdir/$BIN_NAME" "$install_path"
chmod 755 "$install_path"

printf 'glissues installer: installed to %s\n' "$install_path"
case ":$PATH:" in
  *":$INSTALL_DIR:"*)
    ;;
  *)
    printf 'glissues installer: add %s to PATH if needed\n' "$INSTALL_DIR"
    ;;
esac
