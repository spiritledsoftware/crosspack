# PROJECT KNOWLEDGE BASE

**Generated:** 2026-02-24
**Commit:** 9bbb5cb
**Branch:** feat/cli-output-ux

## OVERVIEW
Crosspack is a Rust workspace for a native cross-platform package manager. The CLI crate orchestrates install/upgrade/source-management flows over focused library crates for core models, registry trust, resolver logic, installer state, and security primitives.

## STRUCTURE
```text
./
├── crates/
│   ├── crosspack-cli/        # single runtime entrypoint; all command routing
│   ├── crosspack-installer/  # transaction/state lifecycle + prefix layout
│   ├── crosspack-registry/   # source records, snapshots, signature checks
│   ├── crosspack-resolver/   # dependency graph solve and ordering
│   ├── crosspack-core/       # shared manifest/domain types
│   └── crosspack-security/   # checksum + Ed25519 verification helpers
├── docs/                     # GA behavior specs + non-GA roadmap specs
├── scripts/                  # install/bootstrap and snapshot health automation
└── .github/workflows/        # CI, release, prerelease, registry sync pipelines
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Add/change CLI command behavior | `crates/crosspack-cli/src/main.rs` | Central integration hotspot across all crates |
| Manifest schema or metadata fields | `crates/crosspack-core/src/lib.rs` | Keep docs in sync when fields change |
| Registry trust/snapshot logic | `crates/crosspack-registry/src/lib.rs` | Fingerprint and signature rules fail closed |
| Install/upgrade/uninstall state changes | `crates/crosspack-installer/src/lib.rs` | Receipts, pins, transaction markers, rollback hooks |
| Resolver policy changes | `crates/crosspack-resolver/src/lib.rs` | Constraint solve and deterministic ordering |
| Hash/signature verification | `crates/crosspack-security/src/lib.rs` | Shared by registry and CLI install paths |
| CI/release behavior | `.github/workflows/*.yml` | Docs-only changes are path-ignored in CI |

## CODE MAP
| Symbol Cluster | Type | Location | Refs | Role |
|----------------|------|----------|------|------|
| `Commands` and command handlers | enums/fns | `crates/crosspack-cli/src/main.rs` | high | User-facing command dispatch + output contract |
| install transaction + receipt types | structs/fns | `crates/crosspack-installer/src/lib.rs` | high | Prefix layout, lifecycle state, rollback metadata |
| source records + snapshot update | structs/fns | `crates/crosspack-registry/src/lib.rs` | high | Source management and trust-anchored metadata reads |
| dependency solver core | fns | `crates/crosspack-resolver/src/lib.rs` | medium | Deterministic dependency planning |

## CONVENTIONS
- Workspace crates inherit version/edition/license and most dependencies from root `Cargo.toml`; avoid per-crate drift.
- `crosspack` and `cpk` binaries both map to the same `crates/crosspack-cli/src/main.rs` entrypoint.
- Output mode is contract-sensitive: interactive terminals get rich status; non-interactive output remains deterministic plain text.
- Specs marked v0.4/v0.5 are roadmap design docs unless behavior is explicitly shipped in current code/tests.
- Native package-manager wrapping is out of scope: do not wrap distro package manager commands; Linux support targets cross-distro/self-contained artifacts rather than distro-specific package formats.

## ANTI-PATTERNS (THIS PROJECT)
- Do not claim roadmap specs as GA behavior in CLI/docs.
- Do not bypass registry fingerprint/signature verification in metadata-dependent paths.
- Do not alter deterministic machine-oriented output lines (`transaction_*`, `risk_flags`, `change_*`, update summary format) without coordinated contract update.
- Do not change installer transaction/receipt fields without syncing `docs/install-flow.md` and related specs.

## UNIQUE STYLES
- Security/trust wording is explicit and fail-closed; docs intentionally include operational guardrails.
- Release flow is split: Release Please (version/changelog), then artifact workflows on tags, then registry sync on stable publish.
- Snapshot health is treated as a first-class release gate (`scripts/validate-snapshot-flow.sh`, `scripts/check-snapshot-mismatch-health.sh`).

## COMMANDS
```bash
rustup run stable cargo fmt --all --check
rustup run stable cargo clippy --workspace --all-targets --all-features -- -D warnings
rustup run stable cargo build --workspace --locked
rustup run stable cargo test --workspace
scripts/validate-snapshot-flow.sh
```

## NOTES
- There were no existing repo-local `AGENTS.md` files at generation time.
- Build artifacts under `target/` dominate raw file counts; prefer `git ls-files` for source-aware structure analysis.

## USER PREFERENCES
- Write planning/design documents to `.agents/plans/` instead of `docs/plans/`.
