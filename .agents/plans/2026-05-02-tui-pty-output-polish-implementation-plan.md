# TUI And PTY Output Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stabilize Crosspack's interactive terminal and PTY output by consolidating progress rendering, separating stdout results from stderr progress, and adding explicit output policy tests.

**Architecture:** Keep plain output contracts stable. Move install progress off the bespoke raw escape writer and onto the existing `TerminalRenderer`/`indicatif` path, then add policy and rich-formatting improvements in a second phase. Treat `crates/crosspack-cli/src/render.rs` as the boundary for human terminal presentation.

**Tech Stack:** Rust 2021, `indicatif`, `anstyle`, direct `console` in Phase 2, dev-only `assert_cmd` for captured output tests, dev-only `rexpect` only for a narrow PTY regression, current `crosspack-cli` include-file module layout.

---

## File Structure

- Modify `Cargo.toml`: add workspace dependencies for `console`, `assert_cmd`, and `rexpect`.
- Modify `crates/crosspack-cli/Cargo.toml`: add direct `console` dependency for display-width helpers and `assert_cmd`/`rexpect` dev-dependencies for output regression tests.
- Modify `crates/crosspack-cli/src/main.rs`: remove `InstallProgressMode`, `InstallProgressRenderer`, raw progress line formatting, and install-progress locale probing after replacement coverage exists.
- Modify `crates/crosspack-cli/src/render.rs`: extend `TerminalRenderer`/`TerminalProgress` to support install phase progress, stderr draw targets, progress policy, and width-aware rich tables.
- Modify `crates/crosspack-cli/src/core_flows.rs`: change `install_resolved` to report install phases through the renderer/progress abstraction instead of `InstallProgressRenderer`.
- Modify `crates/crosspack-cli/src/dispatch.rs`: pass output/progress policy into install command execution where needed.
- Modify `crates/crosspack-cli/src/command_flows.rs`: keep existing progress use compatible with the updated renderer; route human lines through progress-safe methods where progress may be active.
- Modify `crates/crosspack-cli/src/tests.rs`: add regression tests for progress policy, stdout/stderr separation decisions, install phase rendering, rich table width behavior, and preserved plain contracts.
- Create `crates/crosspack-cli/tests/cli_output.rs`: integration tests for captured stdout/stderr behavior using `assert_cmd`.

---

## Phase 1: Progress Stabilization

Complete and verify this phase before starting Phase 2. Both phases belong in the same PR, but implementation should proceed one phase at a time.

### Task 1: Lock Current Plain And Progress Policy Behavior

**Files:**

- Modify: `crates/crosspack-cli/src/tests.rs`
- Read only for context: `crates/crosspack-cli/src/main.rs`, `crates/crosspack-cli/src/render.rs`

- [ ] **Step 1: Add failing tests for the target policy split**

Add these tests near the existing output-style tests in `crates/crosspack-cli/src/tests.rs`:

```rust
#[test]
fn output_style_uses_stdout_for_result_formatting() {
    assert_eq!(resolve_output_style(true, false), OutputStyle::Plain);
    assert_eq!(resolve_output_style(true, true), OutputStyle::Rich);
}

#[test]
fn progress_policy_uses_stderr_for_ephemeral_output() {
    assert!(!resolve_progress_enabled(OutputStyle::Plain, true));
    assert!(resolve_progress_enabled(OutputStyle::Rich, true));
    assert!(!resolve_progress_enabled(OutputStyle::Rich, false));
}
```

If `resolve_progress_enabled` does not exist yet, this test should fail to compile. Keep `OutputStyle::Plain` disabling progress in this first phase so redirection remains conservative.

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```bash
cargo test -p crosspack-cli progress_policy_uses_stderr_for_ephemeral_output -- --test-threads=1
```

Expected: compile failure for missing `resolve_progress_enabled`.

- [ ] **Step 3: Add minimal progress policy helper**

Add this helper near `resolve_output_style` in `crates/crosspack-cli/src/main.rs`:

```rust
fn resolve_progress_enabled(style: OutputStyle, stderr_is_tty: bool) -> bool {
    style == OutputStyle::Rich && stderr_is_tty
}

fn current_progress_enabled(style: OutputStyle) -> bool {
    resolve_progress_enabled(style, std::io::stderr().is_terminal())
}
```

- [ ] **Step 4: Run focused tests and verify pass**

Run:

```bash
cargo test -p crosspack-cli progress_policy -- --test-threads=1
```

Expected: the new progress policy test passes.

---

### Task 2: Extend TerminalProgress For Install Phases

**Files:**

- Modify: `crates/crosspack-cli/src/render.rs`
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add tests for install phase progress line formatting**

Replace the old `InstallProgressRenderer`-specific expectations with pure tests for a small formatter in `render.rs`:

```rust
#[test]
fn render_install_phase_message_includes_package_phase_and_step() {
    assert_eq!(
        render_install_phase_message("ripgrep", "download", 2, 7, Some((50, Some(200)))),
        "ripgrep download 2/7 50B/200B (25%)"
    );
}

#[test]
fn render_install_phase_message_handles_unknown_download_total() {
    assert_eq!(
        render_install_phase_message("ripgrep", "download", 2, 7, Some((50, None))),
        "ripgrep download 2/7 50B"
    );
}

#[test]
fn render_install_phase_message_omits_transfer_for_non_download_steps() {
    assert_eq!(
        render_install_phase_message("ripgrep", "verify", 3, 7, None),
        "ripgrep verify 3/7"
    );
}
```

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```bash
cargo test -p crosspack-cli render_install_phase_message -- --test-threads=1
```

Expected: compile failure for missing `render_install_phase_message`.

- [ ] **Step 3: Implement install phase message formatter**

Add this to `crates/crosspack-cli/src/render.rs` near the progress helpers:

```rust
fn render_install_phase_message(
    package: &str,
    phase: &str,
    step: usize,
    total_steps: usize,
    download_progress: Option<(u64, Option<u64>)>,
) -> String {
    let bounded_step = step.min(total_steps);
    let mut message = format!("{package} {phase} {bounded_step}/{total_steps}");
    if let Some((downloaded, total)) = download_progress {
        match total {
            Some(total_bytes) if total_bytes > 0 => {
                let percent = ((downloaded as f64) / (total_bytes as f64) * 100.0)
                    .clamp(0.0, 100.0);
                message.push_str(&format!(" {downloaded}B/{total_bytes}B ({percent:.0}%)"));
            }
            Some(total_bytes) => message.push_str(&format!(" {downloaded}B/{total_bytes}B")),
            None => message.push_str(&format!(" {downloaded}B")),
        }
    }
    message
}
```

- [ ] **Step 4: Add install phase update method**

Extend `TerminalProgress` in `crates/crosspack-cli/src/render.rs` with:

```rust
fn set_install_phase(
    &mut self,
    package: &str,
    phase: &str,
    step: usize,
    total_steps: usize,
    download_progress: Option<(u64, Option<u64>)>,
) {
    self.total = total_steps as u64;
    self.current = step.min(total_steps) as u64;
    let message = render_install_phase_message(
        package,
        phase,
        step,
        total_steps,
        download_progress,
    );

    let Some(progress_bar) = &self.progress_bar else {
        return;
    };

    let safe_total = self.total.max(1);
    progress_bar.set_length(safe_total);
    progress_bar.set_position(self.current.min(safe_total));
    progress_bar.set_message(message);
}
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p crosspack-cli render_install_phase_message -- --test-threads=1
```

Expected: all `render_install_phase_message_*` tests pass.

---

### Task 3: Move Install Flow Onto TerminalProgress

**Files:**

- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add a compile-driving test for removed install progress mode**

Delete or rewrite tests that reference these implementation details:

```rust
resolve_install_progress_mode
current_install_progress_mode
install_progress_frames
format_install_progress_line
InstallProgressRenderer
install_progress_renderer_finish_sequence
should_render_install_progress_update
```

Keep coverage by replacing them with tests from Task 1 and Task 2.

- [ ] **Step 2: Change dispatch to pass progress enabled boolean**

In `crates/crosspack-cli/src/dispatch.rs`, replace:

```rust
let install_progress_mode = current_install_progress_mode(output_style);
```

with:

```rust
let progress_enabled = current_progress_enabled(output_style);
```

Then pass `progress_enabled` into `InstallResolvedOptions` instead of `install_progress_mode`. Adjust the struct definition in `core_flows.rs` accordingly.

- [ ] **Step 3: Update `InstallResolvedOptions`**

In `crates/crosspack-cli/src/core_flows.rs`, change the options struct field from an install-progress enum to a boolean:

```rust
struct InstallResolvedOptions<'a> {
    force_redownload: bool,
    snapshot_id: Option<&'a str>,
    interaction_policy: InstallInteractionPolicy,
    progress_enabled: bool,
}
```

Update all struct initializers in `dispatch.rs`, `command_flows.rs`, and tests to use `progress_enabled`.

- [ ] **Step 4: Replace `InstallProgressRenderer` usage in `install_resolved`**

At the start of `install_resolved`, replace the custom progress creation with:

```rust
const INSTALL_PROGRESS_STEPS: usize = 7;
let output_style = current_output_style();
let renderer = TerminalRenderer::from_style(output_style);
let mut progress = options
    .progress_enabled
    .then(|| renderer.start_progress("install", INSTALL_PROGRESS_STEPS as u64));

if let Some(active_progress) = progress.as_mut() {
    active_progress.set_install_phase(&resolved.manifest.name, "preflight", 1, INSTALL_PROGRESS_STEPS, None);
}
```

Replace each existing `progress.update(...)` call with `if let Some(active_progress) = progress.as_mut() { active_progress.set_install_phase(...); }` using the same phase names and step numbers.

At completion, replace `progress.finish();` with:

```rust
finish_progress(progress);
```

On error paths, rely on `TerminalProgress::drop`/`finish_abandon` behavior if implemented; otherwise explicitly call `finish_abandon` before returning early.

- [ ] **Step 5: Remove old raw progress implementation from `main.rs`**

Delete these definitions from `crates/crosspack-cli/src/main.rs` after all compile errors are addressed:

```rust
enum InstallProgressMode
ASCII_PROGRESS_FRAMES
UNICODE_PROGRESS_FRAMES
locale_looks_utf8
resolve_install_progress_mode
current_install_progress_mode
install_progress_frames
InstallProgressLineState
format_install_progress_line
InstallProgressRenderer
DOWNLOAD_PROGRESS_REDRAW_INTERVAL
install_progress_renderer_finish_sequence
should_render_install_progress_update
```

Also remove now-unused imports such as `Write` if no longer needed in `main.rs`.

- [ ] **Step 6: Run install/progress-focused tests**

Run:

```bash
cargo test -p crosspack-cli install_progress render_install_phase_message -- --test-threads=1
```

Expected: tests pass or compile errors point only to missed references to removed progress types.

---

### Task 4: Make Indicatif Draw To Stderr And Print Safely

**Files:**

- Modify: `crates/crosspack-cli/src/render.rs`
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add renderer construction test for disabled progress**

Add a pure behavior test:

```rust
#[test]
fn terminal_renderer_does_not_create_progress_for_plain_style() {
    let renderer = TerminalRenderer::from_style(OutputStyle::Plain);
    let progress = renderer.start_progress("install", 7);
    assert!(progress.progress_bar.is_none());
}
```

If private fields make direct assertion noisy, expose this test-only helper on `TerminalProgress`:

```rust
#[cfg(test)]
fn has_progress_bar_for_tests(&self) -> bool {
    self.progress_bar.is_some()
}
```

- [ ] **Step 2: Set the progress draw target explicitly**

In `TerminalRenderer::start_progress`, after creating the bar, set its draw target:

```rust
progress_bar.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(12));
```

Remove `enable_steady_tick(Duration::from_millis(80))` for determinate install/update progress bars. Keep steady ticks only for future spinner-only operations that have no step or byte progress.

- [ ] **Step 3: Keep all status printing progress-safe**

Ensure `TerminalProgress::print_line` continues to use:

```rust
progress_bar.println(line);
```

Do not call raw `println!` while progress is active in install/upgrade/update/self-update paths.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p crosspack-cli terminal_renderer -- --test-threads=1
```

Expected: renderer tests pass.

---

## Phase 2: Policy, Width, And Regression Coverage

### Task 5: Add Internal Color And Progress Policy Modes

**Files:**

- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add internal policy enums and tests**

Add tests first:

```rust
#[test]
fn progress_mode_auto_follows_stderr_tty() {
    assert!(resolve_progress_mode(ProgressMode::Auto, OutputStyle::Rich, true));
    assert!(!resolve_progress_mode(ProgressMode::Auto, OutputStyle::Rich, false));
    assert!(!resolve_progress_mode(ProgressMode::Auto, OutputStyle::Plain, true));
}

#[test]
fn progress_mode_always_forces_progress_for_rich_output() {
    assert!(resolve_progress_mode(ProgressMode::Always, OutputStyle::Rich, false));
    assert!(!resolve_progress_mode(ProgressMode::Always, OutputStyle::Plain, false));
}

#[test]
fn progress_mode_never_disables_progress() {
    assert!(!resolve_progress_mode(ProgressMode::Never, OutputStyle::Rich, true));
}
```

Add enum near `OutputStyle`:

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum ProgressMode {
    Auto,
    Always,
    Never,
}
```

Add helper:

```rust
fn resolve_progress_mode(mode: ProgressMode, style: OutputStyle, stderr_is_tty: bool) -> bool {
    match mode {
        ProgressMode::Auto => resolve_progress_enabled(style, stderr_is_tty),
        ProgressMode::Always => style == OutputStyle::Rich,
        ProgressMode::Never => false,
    }
}
```

- [ ] **Step 2: Keep progress policy internal for this PR**

Do not add a public `--progress` flag in this PR. Keep `ProgressMode` internal and use it to make tests and future public API work straightforward. Do not modify `Cli` for progress flags unless a later implementation finding proves the internal policy cannot satisfy the behavior.

Do not add this field now:

```rust
#[arg(long, value_enum, default_value_t = ProgressMode::Auto)]
progress: ProgressMode,
```

Use `ProgressMode::Auto` internally for production paths. Tests may call `resolve_progress_mode` directly for `Always` and `Never` behavior.

- [ ] **Step 3: Defer public color mode and add internal color hook only when styling needs it**

Do not add a public `--color` flag in this plan. If Phase 2 styling work needs color suppression tests, add an internal helper that treats `NO_COLOR` as disabling color rendering and keep public CLI surface unchanged.

---

### Task 6: Make Rich Tables Width-Aware

**Files:**

- Modify: `Cargo.toml` if direct `console` dependency is needed
- Modify: `crates/crosspack-cli/Cargo.toml` if direct `console` dependency is needed
- Modify: `crates/crosspack-cli/src/render.rs`
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add display-width tests**

Add tests near `render_compact_table_rich_aligns_columns`:

```rust
#[test]
fn render_compact_table_rich_uses_display_width_for_unicode() {
    let rows = vec![
        vec!["name".to_string(), "version".to_string()],
        vec!["工具".to_string(), "1.0.0".to_string()],
        vec!["ripgrep".to_string(), "14.1.0".to_string()],
    ];

    assert_eq!(
        render_compact_table(OutputStyle::Rich, &rows),
        vec![
            "name     version".to_string(),
            "工具     1.0.0".to_string(),
            "ripgrep  14.1.0".to_string(),
        ]
    );
}
```

- [ ] **Step 2: Add direct `console` dependency**

At root `Cargo.toml`, add:

```toml
console = "0.16"
```

In `crates/crosspack-cli/Cargo.toml`, add:

```toml
console.workspace = true
```

Use the version already locked transitively. Do not run `cargo update -p console` unless Cargo reports the locked version cannot satisfy the new direct dependency.

- [ ] **Step 3: Update rich table padding**

In `render_compact_table`, replace byte-length width calculation with display width:

```rust
let display_width = console::measure_text_width(cell);
widths[index] = widths[index].max(display_width);
```

When padding a non-final cell, calculate padding from display width:

```rust
let display_width = console::measure_text_width(cell);
line.push_str(cell);
line.push_str(&" ".repeat(width.saturating_sub(display_width)));
```

Keep plain mode unchanged.

- [ ] **Step 4: Run render tests**

Run:

```bash
cargo test -p crosspack-cli render_compact_table -- --test-threads=1
```

Expected: rich table tests pass and plain table tests remain unchanged.

---

### Task 7: Add Command Output And PTY Regression Tests

**Files:**

- Modify: `Cargo.toml` if adding dev dependency at workspace level
- Modify: `crates/crosspack-cli/Cargo.toml`
- Create: `crates/crosspack-cli/tests/cli_output.rs`
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add `assert_cmd` for captured-output assertions**

Add `assert_cmd`:

```toml
[workspace.dependencies]
assert_cmd = "2.0"

[dev-dependencies]
assert_cmd.workspace = true
```

- [ ] **Step 2: Add test that captured stdout has no progress escapes**

Create `crates/crosspack-cli/tests/cli_output.rs`:

```rust
use assert_cmd::Command;

#[test]
fn doctor_stdout_has_no_terminal_control_sequences_when_captured() {
    let output = Command::cargo_bin("crosspack")
        .expect("crosspack binary should build")
        .arg("doctor")
        .output()
        .expect("doctor should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains('\r'), "stdout contained carriage return: {stdout:?}");
    assert!(!stdout.contains("\x1b[2K"), "stdout contained clear-line escape: {stdout:?}");
}
```

- [ ] **Step 3: Add one Unix-only PTY test for redraw-specific behavior**

Add `rexpect` and launch `target/debug/crosspack doctor` or the lightest command that exercises rich output after building. Assert final text does not contain stale duplicated progress frames. Gate the test with `#[cfg(unix)]`.

Add dependencies:

```toml
[workspace.dependencies]
rexpect = "0.6"

[dev-dependencies]
rexpect.workspace = true
```

- [ ] **Step 4: Run output tests**

Run:

```bash
cargo test -p crosspack-cli --test cli_output -- --test-threads=1
```

Expected: integration output tests pass. If no integration test file was created, run the equivalent focused unit tests.

---

### Task 8: Sweep Direct Human Output Call Sites Opportunistically

**Files:**

- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Identify direct prints that overlap active progress**

Search:

```bash
rg -n "println!|eprintln!" crates/crosspack-cli/src/dispatch.rs crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/core_flows.rs
```

Classify each hit as one of:

- Machine contract: leave direct or keep in formatter.
- Shell payload: leave direct.
- Human status while progress may be active: route through `TerminalRenderer` or `TerminalProgress`.
- Human rich formatter output: route through existing style-aware formatter.

- [ ] **Step 2: Change only progress-overlap call sites**

Do not rewrite unrelated machine output. For status emitted while an `Option<TerminalProgress>` is active, use existing helpers:

```rust
print_status_with_progress(renderer, progress.as_ref(), "step", "message");
print_line_with_progress(progress.as_ref(), &line);
```

- [ ] **Step 3: Add regression tests for any formatter touched**

For every formatter changed, add one plain-mode test and one rich-mode test when deterministic.

- [ ] **Step 4: Run CLI tests**

Run:

```bash
cargo test -p crosspack-cli -- --test-threads=1
```

Expected: all CLI tests pass.

---

## Final Verification

- [ ] **Step 1: Format check**

Run:

```bash
cargo fmt --all --check
```

Expected: exits 0.

- [ ] **Step 2: Focused clippy**

Run:

```bash
cargo clippy -p crosspack-cli --all-targets -- -D warnings
```

Expected: exits 0.

- [ ] **Step 3: Focused tests**

Run:

```bash
cargo test -p crosspack-cli
```

Expected: exits 0.

- [ ] **Step 4: Manual PTY smoke test using compiled binary**

Run after building, not through noisy `cargo run`:

```bash
cargo build -p crosspack-cli --bin crosspack
./target/debug/crosspack doctor
```

Expected: Crosspack output is readable; no raw `\r` or `\x1b[2K` artifacts are visible in the final terminal state.

- [ ] **Step 5: Plain redirection smoke test**

Run:

```bash
./target/debug/crosspack doctor > /tmp/crosspack-doctor.out
```

Expected: `/tmp/crosspack-doctor.out` contains no spinner frames, carriage returns, clear-line escapes, or ANSI style sequences.

---

## Commit Strategy

Use small commits if requested by the user:

- `test(cli): lock terminal progress policy`
- `refactor(cli): use indicatif for install progress`
- `feat(cli): add terminal output policy controls`
- `test(cli): cover captured and pty output`

Do not commit without explicit user request.
