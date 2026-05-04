# Modern Terminal UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` or execute task-by-task inline. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add snapshot-based terminal output inspection, then polish Crosspack rich terminal output into a modern package-manager style without changing plain automation contracts.

**Architecture:** Add `insta` snapshots first so visual changes are reviewable as terminal-output artifacts. Add internal/test-only capture controls for deterministic fixture generation. Keep runtime changes concentrated in `crates/crosspack-cli/src/render.rs` and existing formatter seams. Do not add new public CLI flags.

**Tech Stack:** Rust, `insta` for snapshot review, `pretty_assertions` for readable output diffs, existing `anstyle`, `indicatif`, `console`, `assert_cmd`, `rexpect` coverage. `ratatui` remains out of scope.

---

## File Structure

- Modify `.agents/specs/2026-05-03-modern-terminal-ux-spec.md`: approved design record.
- Modify `Cargo.toml`: add workspace dev dependency `insta = "1.47"`.
- Modify `Cargo.toml`: add workspace dev dependency `pretty_assertions = "1.4"`.
- Modify `crates/crosspack-cli/Cargo.toml`: add `insta.workspace = true` and `pretty_assertions.workspace = true` under `[dev-dependencies]`.
- Modify `crates/crosspack-cli/src/render.rs`: status glyphs, section headers, install detail rows, progress template.
- Modify `crates/crosspack-cli/src/tests.rs`: update rich renderer tests and preserve plain tests.
- Create `crates/crosspack-cli/src/snapshots/`: generated `insta` snapshots for terminal renderer galleries.
- Optionally modify `crates/crosspack-cli/src/main.rs`: add hidden/internal UI state dump only if environment-based capture is insufficient.
- Modify command formatter files only if tests reveal direct rich formatting outside renderer helpers.

---

## Task 0: Add Insta Snapshot Harness Before Visual Changes

- [ ] Add `insta = "1.47"` to `[workspace.dependencies]` in `Cargo.toml`.
- [ ] Add `pretty_assertions = "1.4"` to `[workspace.dependencies]` in `Cargo.toml`.
- [ ] Add `insta.workspace = true` and `pretty_assertions.workspace = true` to `crates/crosspack-cli/Cargo.toml` under `[dev-dependencies]`.
- [ ] Import `pretty_assertions::assert_eq` only in output-heavy test modules or local scopes where formatter diffs benefit from it.
- [ ] Add snapshot tests in `crates/crosspack-cli/src/tests.rs` with names prefixed by `terminal_snapshot_`.
- [ ] Snapshot a rich status gallery by joining these lines with `\n`:

```rust
let output = [
    render_status_line(OutputStyle::Rich, "ok", "installed ripgrep 14.1.0"),
    render_status_line(OutputStyle::Rich, "warn", "completion sync skipped"),
    render_status_line(OutputStyle::Rich, "error", "source sync failed"),
    render_status_line(OutputStyle::Rich, "step", "cache: downloaded"),
]
.join("\n");
insta::assert_snapshot!(output);
```

- [ ] Snapshot rich empty state output from `render_empty_state(OutputStyle::Rich, ...)`.
- [ ] Snapshot rich compact table output with ASCII and Unicode package names.
- [ ] Snapshot `format_rich_install_outcome_lines(&sample_install_outcome()).join("\n")` after normalizing fixture paths if needed.
- [ ] Run `cargo test -p crosspack-cli terminal_snapshot -- --test-threads=1` and confirm snapshots are created for review.
- [ ] Review snapshots with `cargo insta review --workspace`; accept only after inspecting the rendered output.

---

## Task 0.5: Add Deterministic Internal UI Capture Controls

- [ ] Add an internal helper that reads `CROSSPACK_INTERNAL_UI_SNAPSHOT=1` and forces snapshot-friendly rich decisions only in tests/development.
- [ ] Add `CROSSPACK_INTERNAL_TERM_WIDTH=<cols>` support for renderer tests that need width-stable output.
- [ ] Add `CROSSPACK_INTERNAL_NO_COLOR=1` support for snapshots that should keep rich layout and glyphs but strip ANSI color.
- [ ] Keep these controls undocumented in user-facing help and README.
- [ ] Add unit tests proving internal capture mode does not change plain output contracts.
- [ ] Add hidden `--dump-ui-state` only if debugging renderer decisions cannot be handled cleanly through tests. If added, mark it hidden in Clap and keep output plain, stable, and internal.

---

## Task 1: Pin Modern Rich Formatter Contracts

- [ ] Add/update tests in `crates/crosspack-cli/src/tests.rs` so rich status lines expect `✓`, `!`, `×`, and `•` instead of ASCII badges.
- [ ] Add/update tests so `render_section_header(UiMode::Interactive, "Installed ripgrep 14.1.0")` returns `Installed ripgrep 14.1.0`, not `== ... ==`.
- [ ] Add/update tests so `render_rich_install_detail_row("step", "archive", "tar.zst")` splits into whitespace-aligned columns and contains no badges or pipes.
- [ ] Run `cargo test -p crosspack-cli render_status_line render_rich_install_detail_row -- --test-threads=1` and confirm failure before implementation.

## Task 2: Implement Renderer Visual Language

- [ ] Update `render_status_line` in `crates/crosspack-cli/src/main.rs` to use rich glyph markers while keeping plain mode unchanged.
- [ ] Update style helpers in `crates/crosspack-cli/src/render.rs` so status, section, and progress colors remain centralized and subdued.
- [ ] Update `render_section_header` to return the bare title in interactive mode.
- [ ] Update `render_rich_install_detail_row` to emit aligned key/value rows without status columns or pipe chrome.
- [ ] Run focused renderer tests and ensure plain tests still pass.

## Task 3: De-Jank Progress Template

- [ ] Update `TerminalRenderer::start_progress` to use a stable prefix plus wide message template.
- [ ] Keep `ProgressDrawTarget::stderr_with_hz(12)` and `finish_and_clear()` behavior.
- [ ] Update install phase messages only if needed for readability, preserving existing phase names and tests.
- [ ] Run progress-related unit tests and captured output tests.

## Task 4: Sweep Rich Hotspots

- [ ] Run formatter tests that cover `search`, `info`, `registry`, `update`, and install outcome rich lines.
- [ ] Update expectations for rich-only glyph output.
- [ ] Leave plain output expectations unchanged.
- [ ] Use shared renderer/status helpers instead of adding ad hoc command formatting.

## Task 5: Verify

- [ ] Run `cargo fmt --all --check`.
- [ ] Run `cargo test -p crosspack-cli render_ -- --test-threads=1`.
- [ ] Run `cargo test -p crosspack-cli --test cli_output -- --test-threads=1`.
- [ ] Run `cargo clippy -p crosspack-cli --all-targets -- -D warnings`.
