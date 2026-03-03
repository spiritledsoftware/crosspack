# CLI Output Mode Hardening Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make all non-install command output mode-safe so TTY sessions show rich output only and non-TTY sessions show plain deterministic output only.

**Architecture:** Centralize output mode guarantees in renderer and command flow boundaries. Keep plain deterministic contracts as the source of truth, and only apply rich decoration when `OutputStyle::Rich` is active. Refactor progress-backed commands to avoid mixed/interleaved output by printing status lines through renderer-aware helpers while progress is active.

**Tech Stack:** Rust, clap, indicatif, cargo test

---

### Task 1: Harden renderer mode boundaries and progress behavior

**Files:**
- Modify: `crates/crosspack-cli/src/render.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

**Step 1: Write failing tests for renderer contracts**

Add tests that fail against current behavior:
- rich progress completion line is not emitted when `total == 0`
- renderer status printing while progress is active does not produce mixed raw-line output behavior
- rich renderer never emits plain-only badges/lines in paths intended to be structured

**Step 2: Run targeted tests to confirm failures**

Run: `rustup run stable cargo test -p crosspack-cli renderer -- --nocapture`

Expected: new tests fail before implementation.

**Step 3: Implement renderer hardening**

In `render.rs`:
- Add progress-safe status/line emission APIs for use while a `TerminalProgress` is active.
- Ensure rich progress completion output is suppressed for zero-work totals.
- Keep plain mode behavior unchanged.

**Step 4: Run renderer-focused tests**

Run: `rustup run stable cargo test -p crosspack-cli renderer`

Expected: renderer tests pass.

### Task 2: Refactor progress-backed command flows to avoid mixed output

**Files:**
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

**Step 1: Write failing tests for progress-backed command output**

Add tests for:
- `run_update_command` zero-source rich mode produces no fake progress completion line
- `run_update_command` still emits deterministic `update summary: ...` line
- rich mode command output for `update`, `upgrade`, `uninstall`, and `self-update` stays renderer-mediated (no mixed plain line artifacts)

**Step 2: Run targeted tests to confirm failures**

Run: `rustup run stable cargo test -p crosspack-cli update_output`

Expected: new tests fail before implementation.

**Step 3: Implement command flow output refactor**

In `command_flows.rs`:
- Replace raw `println!` calls inside active progress loops with renderer/progress-safe printing.
- For zero-work paths, skip progress rendering and print only stable status/summary lines.
- Preserve plain-mode deterministic content exactly (especially `update summary: updated=<n> up-to-date=<n> failed=<n>`).

**Step 4: Run command-flow-focused tests**

Run: `rustup run stable cargo test -p crosspack-cli update_output`

Expected: new tests pass.

### Task 3: Enforce strict TTY split across dispatch and bundle command surfaces

**Files:**
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Modify: `crates/crosspack-cli/src/bundle_flows.rs`
- Modify: `crates/crosspack-cli/src/main.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

**Step 1: Write failing tests for strict mode split**

Add tests that verify:
- when style resolves to `Rich`, user-facing status lines are rich-only
- when style resolves to `Plain`, output remains plain-only without rich adornments
- bundle status output follows style split while raw payload output (bundle document stdout, completion script content) remains undecorated bytes

**Step 2: Run targeted tests to confirm failures**

Run: `rustup run stable cargo test -p crosspack-cli output_style`

Expected: new tests fail before implementation.

**Step 3: Implement strict mode split refactor**

In `dispatch.rs`, `bundle_flows.rs`, and helper functions in `main.rs`:
- Route human-readable status output through style-aware render helpers.
- Ensure no command emits both rich and plain formatting in the same execution mode.
- Keep machine/script payload output raw and deterministic.

**Step 4: Run targeted output style tests**

Run: `rustup run stable cargo test -p crosspack-cli output_style`

Expected: tests pass.

### Task 4: Full verification and cleanup

**Files:**
- Modify: `crates/crosspack-cli/src/tests.rs` (if minor cleanup needed)

**Step 1: Run formatter and lints**

Run: `rustup run stable cargo fmt --all`

Run: `rustup run stable cargo clippy -p crosspack-cli --all-targets -- -D warnings`

Expected: no lint errors.

**Step 2: Run full crate tests**

Run: `rustup run stable cargo test -p crosspack-cli`

Expected: all tests pass.

**Step 3: Manual mode verification for update output**

Run (non-TTY):
- `cargo run -q -p crosspack-cli --bin crosspack -- update`

Run (TTY simulation):
- `script -q -c "cargo run -q -p crosspack-cli --bin crosspack -- update" /dev/null`

Expected:
- non-TTY shows plain lines only
- TTY shows rich presentation only, with no fake zero-work progress completion line.
