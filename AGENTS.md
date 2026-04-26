# AGENTS.md

## Repo Shape

- Rust workspace (`Cargo.toml`, resolver 2) with members under `crates/*`; workspace version/edition/license/deps are centralized at the root.
- `crosspack-cli` builds both binaries, `crosspack` and `cpk`, from `crates/crosspack-cli/src/main.rs`.
- CLI implementation is split with `include!` at the bottom of `main.rs` (`dispatch.rs`, `command_flows.rs`, `core_flows.rs`, `bundle_flows.rs`, `metadata.rs`, `render.rs`, `completion.rs`), so those files share one module scope.
- `registry` is a git submodule (`https://github.com/spiritledsoftware/crosspack-registry.git`), not ordinary in-repo source.
- More specific guidance exists in `crates/AGENTS.md`, `crates/crosspack-cli/AGENTS.md`, `crates/crosspack-installer/AGENTS.md`, and `crates/crosspack-registry/AGENTS.md`.

## Commands

- Full CI-equivalent gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo build --workspace --locked`, `cargo test --workspace`.
- CI also runs `scripts/validate-snapshot-flow.sh` on Ubuntu for non-doc changes; run it for registry/source/snapshot/transaction changes or before release promotion.
- Focus one crate: `cargo test -p crosspack-cli`, `cargo clippy -p crosspack-cli --all-targets -- -D warnings`.
- Focus one test: `cargo test -p crosspack-registry package_versions`; snapshot-flow tests often use `-- --test-threads=1`.
- Exercise the local CLI without installing: `cargo run -p crosspack-cli --bin crosspack -- <command>`.
- Legacy dev registry bypass: `cargo run -p crosspack-cli --bin crosspack -- --registry-root /path/to/registry search ripgrep`.

## Boundaries

- `crosspack-cli`: command parsing, orchestration, terminal/plain output contracts, and command-to-crate wiring.
- `crosspack-core`: manifest/domain structs and serde-facing schemas; keep it free of command behavior and concrete IO/runtime side effects.
- `crosspack-resolver`: dependency graph solve, constraints, and deterministic ordering; keep installer state and terminal formatting out.
- `crosspack-installer`: prefix layout, receipts, pins, transaction markers/journals, rollback/uninstall/cache/completion/GUI/service state.
- `crosspack-registry`: source records, source ordering, snapshot lifecycle, registry index reads, fingerprint/signature gates.
- `crosspack-security`: SHA-256 and Ed25519 helpers; do not duplicate hash/signature verification ad hoc in other crates.

## Contracts To Preserve

- Plain/non-interactive output is the automation contract; rich TTY output must remain additive decoration only.
- Do not change machine-oriented line shapes without coordinated tests/docs: `transaction_preview`, `transaction_summary`, `risk_flags`, `change_*`, `update summary: updated=<n> up-to-date=<n> failed=<n>`.
- Metadata trust fails closed: configured sources require pinned `registry.pub` fingerprint, ready `snapshot.json`, and verified `.toml.sig` sidecars.
- Mutating flows must use installer transaction preflight/state paths; do not bypass active transaction checks, receipts, pins, or rollback metadata.
- Installer path/schema changes are compatibility-sensitive because receipts and state live under the user prefix (`~/.crosspack` on macOS/Linux, `%LOCALAPPDATA%\Crosspack` on Windows).
- Docs under `docs/*-spec.md` can be roadmap/non-GA; README and `docs/architecture.md` are the better shipped-behavior references.

## Release And Ops

- Release Please drives stable releases from Conventional Commits on `main`; it updates `CHANGELOG.md` and the root workspace version.
- Stable artifact workflow is tag-driven for `vX.Y.Z`; prerelease artifacts come from pushes to `release/*` and are tagged `vX.Y.Z-rc.N`.
- Stable release publish triggers registry sync via `scripts/sync-crosspack-registry-release.sh`; it needs `gh`, `git`, `openssl`, `sha256sum`, `awk`, `xxd`, and registry signing secrets.
- `scripts/check-snapshot-mismatch-health.sh` reads `${CROSSPACK_PREFIX:-~/.crosspack}/state/transactions/snapshot-monitor.log` unless `--prefix` or `--log` is passed.
- Repo-local OpenCode config allows external-directory access to `~/.crosspack/**`; avoid assuming other external paths are pre-approved.

## Project Conventions

- Keep Crosspack UX terminology native; avoid Homebrew-specific terms like tap/cask in CLI UX.
- Write agent planning/design docs under `.agents/plans/`, not `docs/plans/`.
- Contributions are licensed `MIT OR Apache-2.0` unless explicitly stated otherwise.

---

This is a living document. Update it as things change.
