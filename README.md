# Crosspack

Native cross-platform package manager built in Rust.

Crosspack is designed to be deterministic, secure by default, and practical for both daily users and contributors:
- deterministic dependency resolution
- verified registry metadata (Ed25519 sidecar signatures)
- SHA-256 artifact verification
- transaction-aware install, upgrade, and uninstall lifecycle

CLI binaries:
- `crosspack` (canonical)
- `cpk` (short alias)

## Why Crosspack

Crosspack exists to provide a native package manager with first-class Windows, macOS, and Linux behavior, without wrapping another ecosystem's package manager.

### Project goals

- Cross-platform parity: one CLI and one install model across major operating systems.
- Deterministic behavior: stable output and predictable resolution and install order.
- Trust-pinned metadata: registry key fingerprint pinning plus fail-closed metadata verification.
- Clear crate boundaries: CLI orchestration separated from focused domain crates.

## GA scope (shipped in v0.3)

The current GA scope is the behavior implemented in this repository today (v0.3 baseline):
- source management with trusted fingerprint pinning (`registry add/list/remove`, `update`)
- strict registry metadata signature verification (`registry.pub` + `<version>.toml.sig`)
- deterministic metadata reads from verified local snapshots
- install/upgrade/uninstall lifecycle with receipts, pins, and transaction recovery commands (`rollback`, `repair`, `doctor`)

Anything described as v0.4/v0.5 in docs is roadmap design work and is **not** part of current GA guarantees.

## Current capabilities

- Search and inspect package metadata from verified local snapshots.
- Configure multiple registry sources with deterministic precedence.
- Install packages with transitive dependency resolution and target selection.
- Enforce per-package version pins.
- Upgrade single packages or all installed roots.
- Uninstall with dependency-aware blocking and orphan pruning.
- Recover transaction state with `rollback`, `repair`, and `doctor`.

## Prerequisites

- Rust stable toolchain.
- Platform tools used by download and extraction paths:
  - Unix: `curl` or `wget`, plus archive tools (`tar`, `unzip`) depending on artifact type.
  - Windows: PowerShell.

## Install (prebuilt binaries)

Latest release in examples below: `v0.0.3`.

### macOS + Linux (one-liner)

```bash
VERSION=v0.0.3; OS=$(uname -s); ARCH=$(uname -m); case "$ARCH" in x86_64|amd64) ARCH=x86_64 ;; arm64|aarch64) ARCH=aarch64 ;; *) echo "Unsupported arch: $ARCH"; exit 1 ;; esac; case "$OS" in Darwin) TARGET="${ARCH}-apple-darwin" ;; Linux) if ldd --version 2>&1 | grep -qi musl; then TARGET="${ARCH}-unknown-linux-musl"; else TARGET="${ARCH}-unknown-linux-gnu"; fi ;; *) echo "Unsupported OS: $OS"; exit 1 ;; esac; URL="https://github.com/spiritledsoftware/crosspack/releases/download/${VERSION}/crosspack-${VERSION}-${TARGET}.tar.gz"; TMP=$(mktemp -d); curl -fsSL "$URL" -o "$TMP/crosspack.tgz"; tar -xzf "$TMP/crosspack.tgz" -C "$TMP"; mkdir -p "$HOME/.crosspack/bin"; install -m 755 "$TMP/crosspack" "$HOME/.crosspack/bin/crosspack"; ln -sf "$HOME/.crosspack/bin/crosspack" "$HOME/.crosspack/bin/cpk"; rm -rf "$TMP"; echo "Installed crosspack to $HOME/.crosspack/bin"
```

### Windows (PowerShell one-liner)

```powershell
$v='v0.0.3'; $u="https://github.com/spiritledsoftware/crosspack/releases/download/$v/crosspack-$v-x86_64-pc-windows-msvc.zip"; $d=Join-Path $env:TEMP 'crosspack-install'; Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force -Path $d | Out-Null; Invoke-WebRequest -Uri $u -OutFile (Join-Path $d 'crosspack.zip'); Expand-Archive -Path (Join-Path $d 'crosspack.zip') -DestinationPath $d -Force; $bin=Join-Path $env:LOCALAPPDATA 'Crosspack\bin'; New-Item -ItemType Directory -Force -Path $bin | Out-Null; Copy-Item (Join-Path $d 'crosspack.exe') (Join-Path $bin 'crosspack.exe') -Force; Copy-Item (Join-Path $d 'crosspack.exe') (Join-Path $bin 'cpk.exe') -Force; Remove-Item $d -Recurse -Force; Write-Host "Installed Crosspack to $bin"
```

After install, add the bin directory to your `PATH`:
- macOS/Linux: `~/.crosspack/bin`
- Windows: `%LOCALAPPDATA%\Crosspack\bin`

Notes:
- Current Windows release artifact is `x86_64-pc-windows-msvc`.
- If your shell does not have `curl`, use `wget` in the macOS/Linux flow.

## Quick Start

### 1) Build and verify CLI

```bash
cargo build --workspace
cargo run -p crosspack-cli -- --help
```

### 2) Bootstrap the trusted default source (`core`)

Before first metadata use, verify the published fingerprint in both channels:

- `docs/trust/core-registry-fingerprint.txt` in this repository.
- Matching GitHub Release note entry for the same `updated_at` and `key_id`.

```bash
cargo run -p crosspack-cli -- registry add core https://github.com/spiritledsoftware/crosspack-registry.git --kind git --priority 100 --fingerprint 65149d198a39db9ecfea6f63d098858ed3b06c118c1f455f84ab571106b830c2
cargo run -p crosspack-cli -- update
cargo run -p crosspack-cli -- registry list
```

For operator and support procedures, see `docs/registry-bootstrap-runbook.md`.

### 3) Discover and install packages

```bash
cargo run -p crosspack-cli -- search ripgrep
cargo run -p crosspack-cli -- info ripgrep
cargo run -p crosspack-cli -- install ripgrep
cargo run -p crosspack-cli -- list
```

### 4) Upgrade, pin, and uninstall

```bash
cargo run -p crosspack-cli -- pin ripgrep@^14
cargo run -p crosspack-cli -- upgrade
cargo run -p crosspack-cli -- uninstall ripgrep
```

### 5) Optional: print PATH setup command

```bash
cargo run -p crosspack-cli -- init-shell
```

Tip: `init-shell` prints the command you can add to your shell profile.

## Legacy `--registry-root` mode

For development and tests, you can bypass configured source snapshots and point directly to a registry root:

```bash
cargo run -p crosspack-cli -- --registry-root /path/to/registry search ripgrep
cargo run -p crosspack-cli -- --registry-root /path/to/registry install ripgrep
```

## Command Reference

| Command | Purpose |
|---|---|
| `search <query>` | Search package names. |
| `info <name>` | Show versions and policy metadata for a package. |
| `install <name[@constraint]> [--target <triple>] [--force-redownload] [--provider <capability=package>]` | Resolve and install a package graph. |
| `upgrade [name[@constraint]] [--provider <capability=package>]` | Upgrade one package or all installed root packages. |
| `pin <name@constraint>` | Pin a package version constraint. |
| `uninstall <name>` | Remove a package when not required by remaining roots and prune orphan dependencies. |
| `list` | List installed packages. |
| `registry add <name> <location> --kind <git\|filesystem> --priority <u32> --fingerprint <64-hex>` | Add a trusted source. |
| `registry list` | List configured sources and snapshot state. |
| `registry remove <name> [--purge-cache]` | Remove a source and optionally purge cached snapshots. |
| `update [--registry <name>]...` | Refresh all or selected source snapshots. |
| `rollback [txid]` | Roll back eligible transaction state. |
| `repair` | Recover stale or failed transaction markers. |
| `doctor` | Show prefix paths and transaction health. |
| `init-shell` | Print shell command to add Crosspack bin directory to `PATH`. |

## Security Model

Crosspack verifies both metadata and artifacts:

- Registry source trust is pinned by SHA-256 fingerprint of `registry.pub`.
- Each manifest requires a detached signature sidecar (`<version>.toml.sig`).
- Metadata-dependent commands fail closed on missing or invalid key or signature material.
- Artifacts are verified with SHA-256 before extraction.
- Install state is tracked via receipts and transaction metadata under the prefix state directory.

### Trusted default source and fingerprint channel

- Official default source name: `core`.
- Official source kind and URL: `git` at `https://github.com/spiritledsoftware/crosspack-registry.git`.
- Official fingerprint distribution channel: `docs/trust/core-registry-fingerprint.txt` plus a matching GitHub Release note entry.
- Bootstrap and rotation troubleshooting: `docs/registry-bootstrap-runbook.md`.

Trust boundary note:
- If the entire registry root content (including `registry.pub`) is compromised, authenticity cannot be guaranteed for that compromised root.

## Install Layout

Crosspack uses a scoped prefix:

```text
<prefix>/
  pkgs/
    <name>/<version>/...
  bin/
  cache/
  state/
```

Default user prefix:
- macOS and Linux: `~/.crosspack`
- Windows: `%LOCALAPPDATA%\Crosspack`

## Workspace Architecture

```text
crates/
  crosspack-cli/        # command routing and user-facing output
  crosspack-core/       # manifest and domain model types
  crosspack-registry/   # index traversal and manifest verification
  crosspack-resolver/   # dependency and version selection
  crosspack-installer/  # install, uninstall, receipt, and pin lifecycle
  crosspack-security/   # checksum and signature verification
```

## Development

Run the same quality gates as CI:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Post-merge and pre-release snapshot-flow check:

```bash
scripts/validate-snapshot-flow.sh
```

## Documentation Map

- `docs/architecture.md` - architecture and module responsibilities.
- `docs/install-flow.md` - install, upgrade, and uninstall lifecycle.
- `docs/registry-spec.md` - source and snapshot model.
- `docs/manifest-spec.md` - manifest schema.
- `docs/source-management-spec.md` - v0.3 source-management design.
- `docs/registry-bootstrap-runbook.md` - trusted default source bootstrap, rotation, and failure recovery.
- `docs/dependency-policy-spec.md` - dependency policy and providers roadmap spec (v0.4 draft, non-GA).
- `docs/transaction-rollback-spec.md` - transaction and recovery roadmap spec (v0.5 draft, non-GA).

## Roadmap Notes

Crosspack is developed in incremental milestones. The current implementation includes core source management, strict metadata verification, and transaction foundations.

Roadmap specs in `docs/` (for example v0.4/v0.5 design docs) are planning documents only and must not be read as shipped GA commitments.

## Contributing

Contributions are welcome. Before opening a PR:

1. Run fmt, clippy, and tests.
2. Keep command semantics and user-facing output deterministic.
3. Update docs whenever command behavior changes.

## License

MIT
