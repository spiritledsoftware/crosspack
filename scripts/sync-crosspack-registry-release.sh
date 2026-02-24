#!/usr/bin/env bash
set -euo pipefail

if [ "${TRACE:-0}" = "1" ]; then
  set -x
fi

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "required command not found: $cmd" >&2
    exit 1
  fi
}

require_cmd gh
require_cmd git
require_cmd openssl
require_cmd sha256sum
require_cmd awk
require_cmd xxd

RELEASE_REPOSITORY="${RELEASE_REPOSITORY:?RELEASE_REPOSITORY is required (for example spiritledsoftware/crosspack)}"
REGISTRY_REPOSITORY="${REGISTRY_REPOSITORY:?REGISTRY_REPOSITORY is required (for example spiritledsoftware/crosspack-registry)}"
RELEASE_TAG="${RELEASE_TAG:?RELEASE_TAG is required (for example v0.0.4)}"
REGISTRY_SIGNING_PRIVATE_KEY_PEM="${REGISTRY_SIGNING_PRIVATE_KEY_PEM:?REGISTRY_SIGNING_PRIVATE_KEY_PEM is required}"

if [[ "$RELEASE_TAG" == *"-rc."* ]]; then
  echo "skipping prerelease tag: ${RELEASE_TAG}" >&2
  exit 0
fi

if [[ ! "$RELEASE_TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "unsupported release tag format: ${RELEASE_TAG}" >&2
  exit 1
fi

VERSION="${RELEASE_TAG#v}"
HOME_URL="https://github.com/${RELEASE_REPOSITORY}"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

release_dir="$workdir/release"
mkdir -p "$release_dir"

checksums_path="$release_dir/SHA256SUMS.txt"
max_wait_seconds="${SYNC_RELEASE_ASSET_MAX_WAIT_SECONDS:-1200}"
poll_interval_seconds="${SYNC_RELEASE_ASSET_POLL_INTERVAL_SECONDS:-20}"
deadline_seconds=$((SECONDS + max_wait_seconds))
attempt=1

echo "waiting for SHA256SUMS.txt from ${RELEASE_REPOSITORY}@${RELEASE_TAG}"
while true; do
  rm -f "$checksums_path"

  if gh release download "$RELEASE_TAG" \
    --repo "$RELEASE_REPOSITORY" \
    --pattern "SHA256SUMS.txt" \
    --dir "$release_dir" \
    --clobber >/dev/null 2>&1 && [ -s "$checksums_path" ]; then
    echo "downloaded SHA256SUMS.txt on attempt ${attempt}"
    break
  fi

  if (( SECONDS >= deadline_seconds )); then
    echo "timed out waiting for SHA256SUMS.txt for ${RELEASE_TAG} after ${max_wait_seconds}s" >&2
    gh release view "$RELEASE_TAG" --repo "$RELEASE_REPOSITORY" >/dev/null 2>&1 || true
    exit 1
  fi

  echo "SHA256SUMS.txt not available yet (attempt ${attempt}); retrying in ${poll_interval_seconds}s"
  sleep "$poll_interval_seconds"
  attempt=$((attempt + 1))
done

declare -a targets=(
  "x86_64-unknown-linux-gnu"
  "aarch64-unknown-linux-gnu"
  "x86_64-unknown-linux-musl"
  "aarch64-unknown-linux-musl"
  "x86_64-apple-darwin"
  "aarch64-apple-darwin"
  "x86_64-pc-windows-msvc"
)

checksum_for_asset() {
  local asset="$1"
  awk -v target="$asset" '$2 == target { print $1; exit }' "$checksums_path"
}

registry_dir="$workdir/registry"
echo "cloning registry repository: ${REGISTRY_REPOSITORY}"
gh repo clone "$REGISTRY_REPOSITORY" "$registry_dir" -- --depth 1

manifest_dir="$registry_dir/index/crosspack"
mkdir -p "$manifest_dir"
manifest_path="$manifest_dir/${VERSION}.toml"
sig_path="$manifest_path.sig"

{
  echo 'name = "crosspack"'
  echo "version = \"${VERSION}\""
  echo 'license = "MIT"'
  echo "homepage = \"${HOME_URL}\""

  for target in "${targets[@]}"; do
    if [[ "$target" == "x86_64-pc-windows-msvc" ]]; then
      archive="zip"
      binary_path="crosspack.exe"
    else
      archive="tar.gz"
      binary_path="crosspack"
    fi

    asset="crosspack-${RELEASE_TAG}-${target}.${archive}"
    sha256="$(checksum_for_asset "$asset")"
    if [ -z "$sha256" ]; then
      echo "checksum not found for release asset: ${asset}" >&2
      exit 1
    fi

    url="${HOME_URL}/releases/download/${RELEASE_TAG}/${asset}"

    echo
    echo '[[artifacts]]'
    echo "target = \"${target}\""
    echo "url = \"${url}\""
    echo "sha256 = \"${sha256}\""
    echo "archive = \"${archive}\""
    echo 'strip_components = 0'
    echo
    echo '[[artifacts.binaries]]'
    echo 'name = "crosspack"'
    echo "path = \"${binary_path}\""

    if [[ "$target" == "x86_64-pc-windows-msvc" ]]; then
      echo
      echo '[[artifacts.binaries]]'
      echo 'name = "cpk"'
      echo "path = \"${binary_path}\""
    fi
  done
} > "$manifest_path"

key_path="$workdir/registry-signing.key"
printf '%s' "$REGISTRY_SIGNING_PRIVATE_KEY_PEM" > "$key_path"
chmod 600 "$key_path"

sig_bin_path="$workdir/signature.bin"
openssl pkeyutl -sign -rawin -inkey "$key_path" -in "$manifest_path" -out "$sig_bin_path"
xxd -p -c 9999 "$sig_bin_path" | tr -d '\n' > "$sig_path"
printf '\n' >> "$sig_path"

pushd "$registry_dir" >/dev/null
if [ -z "$(git status --porcelain -- "index/crosspack/${VERSION}.toml" "index/crosspack/${VERSION}.toml.sig")" ]; then
  echo "registry metadata already up to date for crosspack@${VERSION}"
  exit 0
fi

git config user.name "crosspack-bot"
git config user.email "crosspack-bot@users.noreply.github.com"

git add "index/crosspack/${VERSION}.toml" "index/crosspack/${VERSION}.toml.sig"
git commit -m "chore(registry): add crosspack@${VERSION}"
git push origin HEAD:main
popd >/dev/null

echo "published registry metadata for crosspack@${VERSION} to ${REGISTRY_REPOSITORY}"
