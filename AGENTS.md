# PROJECT KNOWLEDGE BASE

**Generated:** 2026-02-12T21:23:56Z
**Commit:** a63c8a4
**Branch:** main

## OVERVIEW
Crosspack is a Rust workspace for a cross-platform package manager with a single CLI binary and focused supporting crates for core models, registry IO, resolution, installation lifecycle, and security verification.

## STRUCTURE
```text
crosspack/
├── crates/
│   ├── crosspack-cli/        # command wiring + user-facing output
│   ├── crosspack-core/       # manifest/domain model types
│   ├── crosspack-installer/  # install/uninstall/receipt/pin lifecycle
│   ├── crosspack-registry/   # index traversal + manifest validation
│   ├── crosspack-resolver/   # dependency and version selection
│   └── crosspack-security/   # checksum/signature verification
├── docs/                     # architecture and flow specs
└── .github/workflows/ci.yml  # quality gates source of truth
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Add or change CLI command | `crates/crosspack-cli/src/main.rs` | All command routing is centralized here. |
| Update manifest schema | `crates/crosspack-core/src/lib.rs` | Shared data model for all crates. |
| Change install/uninstall behavior | `crates/crosspack-installer/src/lib.rs` | Receipts, pin state, filesystem lifecycle. |
| Adjust registry search/info logic | `crates/crosspack-registry/src/lib.rs` | Signature-checked manifest loading. |
| Change dependency resolution rules | `crates/crosspack-resolver/src/lib.rs` | Backtracking + compatibility selection. |
| Change checksum/signature checks | `crates/crosspack-security/src/lib.rs` | Keep behavior deterministic and explicit. |
| Update workflow docs/specs | `docs/` | Keep command semantics in sync with code. |

## CODE MAP
| Symbol | Type | Location | Refs | Role |
|-------|------|----------|------|------|
| `main` | fn | `crates/crosspack-cli/src/main.rs` | high | Central command dispatcher. |
| `RegistryIndex` | struct | `crates/crosspack-registry/src/lib.rs` | high | Registry root reader + query entrypoint. |
| `resolve_dependency_graph` | fn | `crates/crosspack-resolver/src/lib.rs` | high | Dependency graph solver. |
| `PrefixLayout` | struct | `crates/crosspack-installer/src/lib.rs` | high | Filesystem layout contract. |
| `install_from_artifact` | fn | `crates/crosspack-installer/src/lib.rs` | high | Artifact extraction + install path. |
| `verify_sha256_file` | fn | `crates/crosspack-security/src/lib.rs` | medium | Digest verification utility. |

## CONVENTIONS
- One binary entrypoint: `crosspack-cli` owns all command wiring; other crates stay library-only.
- CI order is fixed and strict: fmt check, clippy (warnings denied), workspace tests.
- Commit messages must follow Conventional Commits: `type(scope): subject` (https://www.conventionalcommits.org/en/v1.0.0/).
- Imports grouped as `std` then external crates then workspace crates.
- Use `Path`/`PathBuf` for filesystem work; avoid string path concatenation.
- Keep user-facing messages deterministic; libraries should stay quiet.

## ANTI-PATTERNS (THIS PROJECT)
- Claiming completion without running CI-equivalent checks (or explicitly stating why not run).
- Using `unwrap()` in production paths.
- Adding noisy output in non-CLI crates.
- Introducing path logic via raw string concatenation.
- Skipping test updates for behavior changes.

## UNIQUE STYLES
- Test names describe behavior in snake_case, often long and explicit.
- Small crates with focused responsibility; heavy logic isolated per domain crate.
- CLI lifecycle output uses stable states: installed / upgraded / uninstalled / not installed / up-to-date.

## COMMANDS
```bash
# quality gates (mirror CI)
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace

# local development
cargo build --workspace
cargo run -p crosspack-cli -- --help
cargo run -p crosspack-cli -- search ripgrep
cargo run -p crosspack-cli -- info ripgrep

# focused test runs
cargo test -p crosspack-cli parse_pin_spec_requires_constraint -- --exact
cargo test -p crosspack-installer uninstall_removes_package_dir_and_receipt
```

## NOTES
- CI executes on Linux/macOS/Windows; preserve cross-platform process/path behavior.
- If command semantics change, update `docs/architecture.md`, `docs/install-flow.md`, and related specs under `docs/`.
- For deep crate-specific guidance, read child knowledge files under `crates/`.
