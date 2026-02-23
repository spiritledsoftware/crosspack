#!/usr/bin/env sh
set -eu

REPO="${CROSSPACK_REPO:-spiritledsoftware/crosspack}"
VERSION="${CROSSPACK_VERSION:-}"
PREFIX="${CROSSPACK_PREFIX:-$HOME/.crosspack}"
BIN_DIR="${CROSSPACK_BIN_DIR:-$PREFIX/bin}"

err() {
  echo "error: $*" >&2
  exit 1
}

download() {
  url="$1"
  out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$out" "$url"
  else
    err "curl or wget is required"
  fi
}

sha256_of() {
  file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$file" | awk '{print $2}'
  else
    err "sha256sum, shasum, or openssl is required for checksum verification"
  fi
}

os="$(uname -s)"
arch="$(uname -m)"

case "$arch" in
  x86_64|amd64) arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *) err "unsupported architecture: $arch" ;;
esac

case "$os" in
  Darwin)
    target="${arch}-apple-darwin"
    ;;
  Linux)
    libc="gnu"
    if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
      libc="musl"
    fi
    target="${arch}-unknown-linux-${libc}"
    ;;
  *)
    err "unsupported operating system: $os"
    ;;
esac

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

if [ -z "$VERSION" ]; then
  release_meta="${tmp_dir}/latest-release.json"
  download "https://api.github.com/repos/${REPO}/releases/latest" "$release_meta"
  VERSION="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$release_meta" | head -n1)"
  [ -n "$VERSION" ] || err "failed to resolve latest release tag from GitHub API"
fi

asset="crosspack-${VERSION}-${target}.tar.gz"
base_url="https://github.com/${REPO}/releases/download/${VERSION}"

echo "==> Downloading ${asset}"
download "${base_url}/${asset}" "${tmp_dir}/${asset}"
download "${base_url}/SHA256SUMS.txt" "${tmp_dir}/SHA256SUMS.txt"

expected="$(awk -v f="$asset" '$2 == f { print $1 }' "${tmp_dir}/SHA256SUMS.txt")"
[ -n "$expected" ] || err "checksum for ${asset} not found in SHA256SUMS.txt"
actual="$(sha256_of "${tmp_dir}/${asset}")"

if [ "$expected" != "$actual" ]; then
  err "checksum mismatch for ${asset} (expected ${expected}, got ${actual})"
fi

echo "==> Installing to ${BIN_DIR}"
mkdir -p "$BIN_DIR"
tar -xzf "${tmp_dir}/${asset}" -C "$tmp_dir"

if command -v install >/dev/null 2>&1; then
  install -m 755 "${tmp_dir}/crosspack" "${BIN_DIR}/crosspack"
else
  cp "${tmp_dir}/crosspack" "${BIN_DIR}/crosspack"
  chmod 755 "${BIN_DIR}/crosspack"
fi

if command -v ln >/dev/null 2>&1; then
  ln -sf "${BIN_DIR}/crosspack" "${BIN_DIR}/cpk"
else
  cp "${BIN_DIR}/crosspack" "${BIN_DIR}/cpk"
fi

echo "Installed crosspack (${VERSION}) to ${BIN_DIR}"
echo "Add ${BIN_DIR} to PATH if needed."
