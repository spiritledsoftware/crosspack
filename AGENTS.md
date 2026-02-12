# AGENTS.md

Guidance for coding agents working in `crosspack`.

## Project Snapshot

- Language: Rust (workspace, edition 2021).
- Package manager type: native cross-platform system package manager.
- Workspace crates:
  - `crates/crosspack-cli`
  - `crates/crosspack-core`
  - `crates/crosspack-installer`
  - `crates/crosspack-registry`
  - `crates/crosspack-resolver`
  - `crates/crosspack-security`
- CI runs on Linux/macOS/Windows.

## Source of Truth for Quality Gates

Mirror `.github/workflows/ci.yml`:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace`

Do not claim completion without running these (or explicitly stating what could not be run).

## Build, Lint, and Test Commands

Run from repo root.

### Build

- Build all crates: `cargo build --workspace`
- Build release binaries: `cargo build --workspace --release`
- Build one crate: `cargo build -p crosspack-cli`

### Format and Lint

- Format in-place: `cargo fmt --all`
- Format check only: `cargo fmt --all --check`
- Clippy all crates (strict):
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Clippy one crate:
  `cargo clippy -p crosspack-installer --all-targets --all-features -- -D warnings`

### Tests

- All tests: `cargo test --workspace`
- One crate: `cargo test -p crosspack-cli`
- One test target file (Rust test module/bin target):
  `cargo test -p crosspack-security --lib`
- Single test by name substring:
  `cargo test -p crosspack-installer uninstall_removes_package_dir_and_receipt`
- Single exact test name:
  `cargo test -p crosspack-cli parse_pin_spec_requires_constraint -- --exact`
- With output (for debugging):
  `cargo test -p crosspack-installer uninstall_parse_failure_preserves_files -- --exact --nocapture`

### Running CLI locally

- Help: `cargo run -p crosspack-cli -- --help`
- Search: `cargo run -p crosspack-cli -- search ripgrep`
- Info: `cargo run -p crosspack-cli -- info ripgrep`
- Pin: `cargo run -p crosspack-cli -- pin 'ripgrep@^14'`
- Upgrade one: `cargo run -p crosspack-cli -- upgrade ripgrep`

## Coding Conventions

Follow existing patterns already in this repository.

### Imports

- Group imports in this order:
  1) `std` imports
  2) external crates
  3) workspace crates
- Keep imports explicit; avoid wildcard imports.
- Remove unused imports; clippy treats warnings as errors in CI.

### Formatting

- Always run `cargo fmt --all` after edits.
- Keep function signatures and chained method calls rustfmt-friendly.
- Prefer early returns over deep nesting.

### Types and Data Modeling

- Use strong domain structs/enums for state and outcomes.
  - Examples in codebase: `InstallReceipt`, `UninstallStatus`, `UninstallResult`, `ArchiveType`.
- Prefer `Option<T>` for truly optional fields; avoid sentinel strings.
- Use `Path`/`PathBuf` for filesystem paths, never raw string concatenation.
- Keep public APIs explicit and small.

### Naming

- `snake_case` for functions, variables, modules.
- `PascalCase` for structs/enums/traits.
- Use descriptive names for operations (`resolve_install`, `install_resolved`, `uninstall_package`).
- Test names should describe behavior, not implementation details.

### Error Handling

- Use `anyhow::Result<T>` for fallible operations.
- Add context to IO/process errors with `Context`/`with_context`.
  - Include key identifiers and paths in messages.
- Prefer actionable errors over generic ones.
- Do not use `unwrap()` in production paths.
- `expect()` is acceptable in tests with clear messages.

### Cross-Platform Behavior

- Prefer `std::fs` and `PathBuf` APIs for portability.
- Guard platform-specific behavior with `cfg!(windows)` as done in CLI/installer.
- Preserve Windows-safe behavior around path handling and process invocation.

### CLI and User-Facing Output

- Keep messages concise and deterministic.
- Print clear states for lifecycle commands:
  - installed / upgraded / uninstalled / not installed / up-to-date.
- Avoid noisy output in library crates; output belongs in CLI crate.

### Testing Style

- Add or update tests for all behavior changes.
- Prefer unit tests near changed logic (`#[cfg(test)]` module in same file).
- Use red-green flow:
  1) add failing test,
  2) implement minimal fix,
  3) run targeted tests,
  4) run workspace tests before finalizing.
- Keep tests deterministic and local (no network dependency unless explicitly integration-tested).

### Dependency and Workspace Hygiene

- Prefer workspace dependencies in `Cargo.toml` when possible.
- Keep crate responsibilities separated:
  - `core`: shared models
  - `registry`: index IO/parsing
  - `installer`: filesystem/state lifecycle
  - `security`: checksums/signature-related logic
  - `cli`: command wiring and user interaction

## Files and Docs to Update When Behavior Changes

- Update docs when command semantics change:
  - `docs/architecture.md`
  - `docs/install-flow.md`
  - and any spec files under `docs/`
- Keep examples and command usage consistent with current CLI flags/behavior.

## Team Workflow Preferences

- Commit immediately after finishing each feature implementation (after verification passes).
- If an agent says it will "note" a standing preference, record it here in `AGENTS.md` rather than only acknowledging it in chat.

### Mandatory Note-Taking Protocol

- Treat any user statement matching "always", "never", "from now on", "preference", "when X do Y", or correction of agent behavior as a standing instruction candidate.
- If the instruction is expected to apply beyond the current single task, update `AGENTS.md` in the same session before sending the final response.
- Do not say "noted" unless the `AGENTS.md` edit is already made.
- In the response where the note is recorded, include the exact file path (`AGENTS.md`) and what was added.
- If uncertain whether an instruction is standing or one-off, default to recording it in `AGENTS.md`.

### Completion Checklist (Before Final Response)

- Confirm whether any new standing preference appeared during the conversation.
- If yes, update `AGENTS.md` first, then respond.
- If no, avoid claiming that anything was "noted".

---

This file is yours, feel free to update it as you learn more.
