#!/usr/bin/env sh
set -eu

REPO="${CROSSPACK_REPO:-spiritledsoftware/crosspack}"
VERSION="${CROSSPACK_VERSION:-}"
PREFIX="${CROSSPACK_PREFIX:-$HOME/.crosspack}"
BIN_DIR="${CROSSPACK_BIN_DIR:-$PREFIX/bin}"
CORE_NAME="${CROSSPACK_CORE_NAME:-core}"
CORE_URL="${CROSSPACK_CORE_URL:-https://github.com/spiritledsoftware/crosspack-registry.git}"
CORE_KIND="${CROSSPACK_CORE_KIND:-git}"
CORE_PRIORITY="${CROSSPACK_CORE_PRIORITY:-100}"
CORE_FINGERPRINT="${CROSSPACK_CORE_FINGERPRINT:-}"
TRUST_BULLETIN_URL="${CROSSPACK_TRUST_BULLETIN_URL:-https://raw.githubusercontent.com/${REPO}/main/docs/trust/core-registry-fingerprint.txt}"
SHELL_SETUP_OPT_OUT="${CROSSPACK_NO_SHELL_SETUP:-0}"

SHELL_SETUP_BEGIN="# >>> crosspack shell setup >>>"
SHELL_SETUP_END="# <<< crosspack shell setup <<<"

err() {
  echo "error: $*" >&2
  exit 1
}

warn() {
  echo "warning: $*" >&2
}

is_hex64() {
  value="$1"
  [ "${#value}" -eq 64 ] || return 1
  normalized="$(printf "%s" "$value" | tr 'A-F' 'a-f')"
  case "$normalized" in
    *[!0-9a-f]*|'') return 1 ;;
  esac
  return 0
}

assert_https_url() {
  url="$1"
  case "$url" in
    https://*) return 0 ;;
    *) err "trust bulletin URL must use https: ${url}" ;;
  esac
}

read_bulletin_value() {
  key="$1"
  file="$2"
  awk -F '=' -v key="$key" '
    {
      raw_key = $1
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", raw_key)
      if (raw_key == key) {
        value = $2
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
        gsub(/^"|"$/, "", value)
        print value
        found += 1
      }
    }
    END {
      if (found != 1) {
        exit 1
      }
    }
  ' "$file"
}

resolve_core_fingerprint() {
  if [ -n "$CORE_FINGERPRINT" ]; then
    if ! is_hex64 "$CORE_FINGERPRINT"; then
      err "CROSSPACK_CORE_FINGERPRINT must be 64 hex characters"
    fi
    echo "$CORE_FINGERPRINT"
    return 0
  fi

  assert_https_url "$TRUST_BULLETIN_URL"

  bulletin_path="${tmp_dir}/core-registry-fingerprint.txt"
  if ! download "$TRUST_BULLETIN_URL" "$bulletin_path"; then
    err "failed fetching trust bulletin from ${TRUST_BULLETIN_URL}; set CROSSPACK_CORE_FINGERPRINT to override"
  fi

  bulletin_source="$(read_bulletin_value source "$bulletin_path")" || err "trust bulletin is missing a unique 'source' key"
  bulletin_kind="$(read_bulletin_value kind "$bulletin_path")" || err "trust bulletin is missing a unique 'kind' key"
  bulletin_url="$(read_bulletin_value url "$bulletin_path")" || err "trust bulletin is missing a unique 'url' key"
  bulletin_fingerprint="$(read_bulletin_value fingerprint_sha256 "$bulletin_path")" || err "trust bulletin is missing a unique 'fingerprint_sha256' key"

  [ "$bulletin_source" = "$CORE_NAME" ] || err "trust bulletin source mismatch (expected ${CORE_NAME}, got ${bulletin_source})"
  [ "$bulletin_kind" = "$CORE_KIND" ] || err "trust bulletin kind mismatch (expected ${CORE_KIND}, got ${bulletin_kind})"
  [ "$bulletin_url" = "$CORE_URL" ] || err "trust bulletin URL mismatch (expected ${CORE_URL}, got ${bulletin_url})"
  if ! is_hex64 "$bulletin_fingerprint"; then
    err "trust bulletin fingerprint must be 64 hex characters"
  fi

  echo "$bulletin_fingerprint"
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

print_manual_shell_setup_hints() {
  echo "Manual shell setup:"
  echo "  ${BIN_DIR}/crosspack completions <bash|zsh|fish> > ${PREFIX}/share/completions/crosspack.<shell>"
  echo "  add ${BIN_DIR} to PATH in your shell profile"
}

upsert_profile_block() {
  profile_path="$1"
  block_path="$2"
  filtered_path="${tmp_dir}/profile-filtered-$$"

  if ! awk -v begin="${SHELL_SETUP_BEGIN}" -v end="${SHELL_SETUP_END}" '
    $0 == begin { in_block = 1; next }
    $0 == end { in_block = 0; next }
    in_block == 0 { print }
  ' "${profile_path}" > "${filtered_path}"; then
    rm -f "${filtered_path}"
    return 1
  fi

  if [ -s "${filtered_path}" ]; then
    if ! printf "\n" >> "${filtered_path}"; then
      rm -f "${filtered_path}"
      return 1
    fi
  fi

  if ! cat "${block_path}" >> "${filtered_path}"; then
    rm -f "${filtered_path}"
    return 1
  fi

  # Write via redirection so symlinked profiles (e.g. ~/.zshrc -> dotfiles repo)
  # keep the symlink inode and update the target file contents in place.
  if ! cat "${filtered_path}" > "${profile_path}"; then
    rm -f "${filtered_path}"
    return 1
  fi

  rm -f "${filtered_path}"
  return 0
}

configure_shell_setup() {
  if [ "${SHELL_SETUP_OPT_OUT}" = "1" ]; then
    echo "Skipping shell setup because CROSSPACK_NO_SHELL_SETUP=1"
    return 0
  fi

  shell_name="${SHELL:-}"
  shell_name="${shell_name##*/}"
  profile_path=""
  completion_extension=""
  block_path="${tmp_dir}/shell-setup-block"
  completion_dir="${PREFIX}/share/completions"
  completion_path=""

  case "${shell_name}" in
    bash)
      profile_path="${HOME}/.bashrc"
      completion_extension="bash"
      ;;
    zsh)
      profile_path="${HOME}/.zshrc"
      completion_extension="zsh"
      ;;
    fish)
      profile_path="${HOME}/.config/fish/config.fish"
      completion_extension="fish"
      ;;
    *)
      warn "automatic shell setup skipped: unsupported or unknown shell '${SHELL:-unknown}'"
      print_manual_shell_setup_hints
      return 0
      ;;
  esac

  completion_path="${completion_dir}/crosspack.${completion_extension}"
  if ! mkdir -p "${completion_dir}"; then
    warn "failed creating completion directory at ${completion_dir}"
    print_manual_shell_setup_hints
    return 0
  fi

  if ! "${BIN_DIR}/crosspack" completions "${shell_name}" > "${completion_path}"; then
    warn "failed generating ${shell_name} completion script at ${completion_path}"
    print_manual_shell_setup_hints
    return 0
  fi

  profile_dir="$(dirname "${profile_path}")"
  if ! mkdir -p "${profile_dir}"; then
    warn "failed creating profile directory at ${profile_dir}"
    print_manual_shell_setup_hints
    return 0
  fi

  if [ ! -f "${profile_path}" ]; then
    if ! touch "${profile_path}"; then
      warn "failed creating shell profile at ${profile_path}"
      print_manual_shell_setup_hints
      return 0
    fi
  fi

  case "${shell_name}" in
    bash|zsh)
      cat > "${block_path}" <<EOF
${SHELL_SETUP_BEGIN}
if [ -d "${BIN_DIR}" ] && [ ":\$PATH:" != *":${BIN_DIR}:"* ]; then
  export PATH="${BIN_DIR}:\$PATH"
fi
if [ -f "${completion_path}" ]; then
  . "${completion_path}"
fi
${SHELL_SETUP_END}
EOF
      ;;
    fish)
      cat > "${block_path}" <<EOF
${SHELL_SETUP_BEGIN}
if test -d "${BIN_DIR}"
    if not contains -- "${BIN_DIR}" \$PATH
        set -gx PATH "${BIN_DIR}" \$PATH
    end
end
if test -f "${completion_path}"
    source "${completion_path}"
end
${SHELL_SETUP_END}
EOF
      ;;
  esac

  if ! upsert_profile_block "${profile_path}" "${block_path}"; then
    warn "failed updating shell profile at ${profile_path}"
    print_manual_shell_setup_hints
    return 0
  fi

  echo "Configured ${shell_name} shell profile: ${profile_path}"
  echo "Installed ${shell_name} completions: ${completion_path}"
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

echo "==> Configuring default registry source (${CORE_NAME})"
resolved_core_fingerprint="$(resolve_core_fingerprint)"
if "${BIN_DIR}/crosspack" registry add "${CORE_NAME}" "${CORE_URL}" --kind "${CORE_KIND}" --priority "${CORE_PRIORITY}" --fingerprint "${resolved_core_fingerprint}" >/dev/null 2>&1; then
  echo "Added registry source '${CORE_NAME}'"
else
  if "${BIN_DIR}/crosspack" registry list 2>/dev/null | grep -q "${CORE_NAME}"; then
    echo "Registry source '${CORE_NAME}' already present"
  else
    err "failed to configure registry source '${CORE_NAME}'"
  fi
fi

"${BIN_DIR}/crosspack" update >/dev/null

configure_shell_setup

echo "Installed crosspack (${VERSION}) to ${BIN_DIR}"
echo "Configured registry source '${CORE_NAME}' and refreshed snapshots."
echo "Add ${BIN_DIR} to PATH if needed."
