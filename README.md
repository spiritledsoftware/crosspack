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
- Install package-declared shell completion files (bash/zsh/fish/powershell) into Crosspack-managed completion directories.
- Automatic CLI output mode: rich lifecycle/status output on interactive terminals, plain deterministic output when non-interactive (for scripts/pipes).
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

Use the install scripts for clean one-liners:

### macOS + Linux

```bash
curl -fsSL https://raw.githubusercontent.com/spiritledsoftware/crosspack/main/scripts/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/spiritledsoftware/crosspack/main/scripts/install.ps1 | iex
```

By default, both scripts install the latest GitHub release.

Optional version pinning:
- macOS/Linux: set `CROSSPACK_VERSION` before running the one-liner.
- Windows: download `scripts/install.ps1` and run it with `-Version <tag>`.

Both scripts also bootstrap the trusted default `core` registry source and run `crosspack update` automatically after install.

By default, installers also attempt shell setup:
- macOS/Linux: detect active shell (`bash`, `zsh`, or `fish`) from `$SHELL`, write completions under `<prefix>/share/completions/`, and upsert one managed block in:
  - `~/.bashrc`
  - `~/.zshrc`
  - `~/.config/fish/config.fish`
- Windows: write PowerShell completions under `<prefix>\share\completions\crosspack.ps1` and upsert one managed block in `$PROFILE.CurrentUserCurrentHost`.

Opt-out controls:
- macOS/Linux: set `CROSSPACK_NO_SHELL_SETUP=1`.
- Windows: run installer with `-NoShellSetup`.

If shell setup cannot run (unsupported shell or profile write issue), install still succeeds and prints manual commands.

Package completion file note:
- Package-declared completion files are populated on install/upgrade/reinstall of that package. Existing installed packages may need `crosspack upgrade <name>` (or reinstall) to populate new completion assets.

After install, verify the bin directory is in your `PATH`:
- macOS/Linux default bin dir: `~/.crosspack/bin`
- Windows default bin dir: `%LOCALAPPDATA%\Crosspack\bin`

Notes:
- Install scripts verify artifact SHA-256 against release `SHA256SUMS.txt`.
- Current Windows release artifact is `x86_64-pc-windows-msvc`.

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
cargo run -p crosspack-cli -- install ripgrep --dry-run
cargo run -p crosspack-cli -- list
```

### 4) Upgrade, pin, and uninstall

```bash
cargo run -p crosspack-cli -- pin ripgrep@^14
cargo run -p crosspack-cli -- upgrade
cargo run -p crosspack-cli -- upgrade --dry-run
cargo run -p crosspack-cli -- uninstall ripgrep
```

### 5) Optional: print shell completion script

```bash
cargo run -p crosspack-cli -- completions bash
```

Tip: `completions` targets the canonical `crosspack` binary name.
Tip: generated Crosspack scripts include loader logic for package-declared completion files under `<prefix>/share/completions/packages/<shell>/`.

### 6) Optional: print shell setup snippet (PATH + completion loader)

```bash
cargo run -p crosspack-cli -- init-shell --shell zsh
```

Tip: `init-shell` auto-detects shell when `--shell` is omitted; fallback is `bash` on Unix and `powershell` on Windows.

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
| `install <name[@constraint]> [--target <triple>] [--dry-run] [--force-redownload] [--provider <capability=package>]` | Resolve and install a package graph. `--dry-run` prints a deterministic transaction preview without mutating state. |
| `upgrade [name[@constraint]] [--dry-run] [--provider <capability=package>]` | Upgrade one package or all installed root packages. `--dry-run` prints a deterministic transaction preview without mutating state. |
| `pin <name@constraint>` | Pin a package version constraint. |
| `uninstall <name>` | Remove a package when not required by remaining roots and prune orphan dependencies. |
| `list` | List installed packages. |
| `registry add <name> <location> --kind <git\|filesystem> --priority <u32> --fingerprint <64-hex>` | Add a trusted source. |
| `registry list` | List configured sources and snapshot state. |
| `registry remove <name> [--purge-cache]` | Remove a source and optionally purge cached snapshots. |
| `update [--registry <name>]...` | Refresh all or selected source snapshots. |
| `self-update [--dry-run] [--force-redownload]` | Refresh configured source snapshots, then install the latest `crosspack` package. |
| `rollback [txid]` | Roll back eligible transaction state. |
| `repair` | Recover stale or failed transaction markers. |
| `doctor` | Show prefix paths and transaction health. |
| `version` / `--version` | Print the Crosspack CLI version. |
| `completions <bash\|zsh\|fish\|powershell>` | Print shell completion script for the canonical `crosspack` binary, including package completion loader block. |
| `init-shell [--shell <bash\|zsh\|fish\|powershell>]` | Print shell setup snippet that adds Crosspack bin directory to `PATH` and loads Crosspack/package completion scripts. |

Output contract notes:
- Human-facing lifecycle commands automatically use rich status badges on interactive terminals.
- Non-interactive usage (for example pipes/redirects) stays plain and deterministic.
- Machine-oriented lines remain unchanged, including dry-run `transaction_preview` / `transaction_summary` / `risk_flags` / `change_*` records and `update summary: updated=<n> up-to-date=<n> failed=<n>`.

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
cargo build --workspace --locked
cargo test --workspace
```

Post-merge and pre-release snapshot-flow check:

```bash
scripts/validate-snapshot-flow.sh
```

## Release Automation

Crosspack release metadata is automated from Conventional Commits on `main`:

- `.github/workflows/release-please.yml` uses GitHub App auth via `CROSSPACK_BOT_APP_ID` (repository variable) and `CROSSPACK_BOT_APP_PRIVATE_KEY` (repository secret) so created tags trigger downstream artifact workflows.
- `.github/workflows/release-please.yml` opens/updates release PRs that bump workspace version and update `CHANGELOG.md`.
- Merging the release PR creates the stable tag (`vX.Y.Z`) and GitHub release metadata.
- `.github/workflows/release-artifacts.yml` builds multi-platform artifacts for stable tags and uploads `SHA256SUMS.txt`.
- `.github/workflows/prerelease-artifacts.yml` builds prerelease (`vX.Y.Z-rc.N`) artifacts automatically on `release/*` branch pushes.

Version bump rules:
- `fix:` -> patch
- `feat:` -> minor
- `BREAKING CHANGE:` footer -> major

Dependency maintenance automation:
- `.github/dependabot.yml` opens weekly grouped dependency update PRs (Cargo + GitHub Actions).
- `.github/workflows/dependency-review.yml` checks pull requests for high-severity dependency risk deltas.

## Documentation Map

- `docs/architecture.md` - architecture and module responsibilities.
- `docs/install-flow.md` - install, upgrade, and uninstall lifecycle.
- `docs/registry-spec.md` - source and snapshot model.
- `docs/manifest-spec.md` - manifest schema.
- `docs/source-management-spec.md` - v0.3 source-management design.
- `docs/registry-bootstrap-runbook.md` - trusted default source bootstrap, rotation, and failure recovery.
- `docs/release-checklist.md` - release and prerelease operator checklist with rollback paths.
- `docs/contributor-playbook.md` - contributor workflow and launch runbook.
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
4. Unless explicitly stated otherwise, contributions are licensed under `MIT OR Apache-2.0`.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
