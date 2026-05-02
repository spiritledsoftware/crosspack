# TUI And PTY Output Polish Spec

## Goal

Make Crosspack's interactive terminal output feel stable, readable, and predictable in real terminals and PTYs while preserving plain stdout contracts for automation.

## Background

Crosspack already has an output split:

- `OutputStyle::Plain` is the automation contract.
- `OutputStyle::Rich` adds terminal-only decoration.
- `TerminalRenderer` and `TerminalProgress` live in `crates/crosspack-cli/src/render.rs`.
- Install progress still uses a separate `InstallProgressRenderer` in `crates/crosspack-cli/src/main.rs` that writes raw `\r\x1b[2K` sequences to stdout.

That split is the main source of PTY jank. Progress rendering is duplicated, ephemeral output can land on stdout, and table formatting uses byte length instead of terminal display width.

## Non-Negotiable Output Contracts

Plain/non-interactive output remains the stable machine contract. This work must not change these line shapes without a separate coordinated contract update:

- `transaction_preview`
- `transaction_summary`
- `risk_flags`
- `change_add`
- `change_remove`
- `change_replace`
- `change_transition`
- `update summary: updated=<n> up-to-date=<n> failed=<n>`
- Registry add/list/remove machine-oriented fields
- Existing shell snippet output from `init-shell`

Rich output may change as long as it is additive and terminal-only.

## Scope

Included:

- Consolidate install progress onto the existing `indicatif`-based renderer path.
- Put ephemeral progress on stderr, not stdout.
- Keep stdout clean for command results and automation.
- Add explicit progress/color policy hooks so PTY and CI behavior are deterministic.
- Improve rich table rendering with terminal display width instead of byte length.
- Add PTY-oriented regression coverage for progress and plain-output behavior.
- Document recommended terminal libraries and the point at which a real full-screen TUI would become justified.

Not included:

- Full-screen `ratatui` interface.
- Interactive prompts or guided workflows.
- Command behavior changes, resolver changes, registry trust changes, installer state changes, or transaction model changes.
- Rewriting all CLI output in one pass.

## Phased Delivery

### Phase 1: Progress Stabilization

Replace `InstallProgressRenderer` with `indicatif` support in `TerminalRenderer`/`TerminalProgress`.

Expected behavior:

- Install, upgrade, update, and self-update progress use one progress implementation.
- Progress renders only when stderr is interactive or when explicitly forced by policy.
- Plain stdout remains free of spinner frames, carriage returns, cursor clear sequences, and progress bars.
- Status lines printed while progress is active use progress-safe printing.
- Failed or interrupted progress clears cleanly.
- Successful progress may leave at most one final rich summary line, never a stream of redraw frames.

### Phase 2: Output Policy And Rich Formatting

Make terminal affordances explicit and broaden polish safely.

Expected behavior:

- Add internal progress policy plumbing first. Do not add a public `--progress` flag unless implementation reveals a concrete user-facing need that cannot be handled by TTY detection and environment-sensitive tests.
- Honor `NO_COLOR` for color suppression.
- Honor `CLICOLOR_FORCE` or explicit `always` for forced color/progress in tests and PTYs.
- Rich table alignment uses terminal display width.
- Long rich-mode cells are clamped or truncated to avoid ugly wrapping.
- Plain-mode tables remain tab-separated where they are currently tab-separated.
- Human status output increasingly routes through `TerminalRenderer`; machine contract lines remain direct or contract-specific formatters.

## Recommended Libraries

Use:

- `indicatif`: canonical progress/spinner implementation. Use `ProgressBar`, `ProgressStyle`, `ProgressDrawTarget::stderr_with_hz(...)`, `finish_and_clear`, and progress-safe printing.
- `anstyle`: keep as the core style representation.
- `anstream`: use when writing color-choice-aware stdout/stderr streams becomes necessary.
- `console`: use for terminal width, display width, truncation, and term capability helpers. Make it a direct `crosspack-cli` dependency in Phase 2 so table formatting does not rely on a transitive dependency.
- `assert_cmd`: use for captured stdout/stderr command regression tests.
- `rexpect`: use for the narrow PTY smoke regression if captured-output tests do not reproduce the issue class. Keep the PTY test limited to Linux/macOS if Windows support is not available.

Avoid for this work:

- `ratatui`: too heavy unless Crosspack adds a full-screen dashboard or interactive package browser.
- `crossterm`: not needed unless Crosspack needs raw mode, alternate screen, keyboard input, or custom cursor control.
- Prompt libraries such as `dialoguer` or `inquire`: not needed, and prompts would need careful non-interactive policy design.

## Architecture

### Output Policy

Separate result formatting from terminal affordances:

- `OutputStyle`: controls plain vs rich line content.
- `ProgressMode`: controls whether ephemeral progress can render.
- `ColorMode`: controls ANSI styling emission.

Do not make `OutputStyle::Rich` depend on both stdout and stderr being TTY forever. Progress should primarily depend on stderr. Result formatting can depend on stdout. This allows clean redirection patterns such as:

```bash
crosspack install ripgrep >install.log
```

In that case progress may remain on stderr, while stdout stays machine-friendly.

### Renderer Boundary

Keep rich terminal behavior centralized in `crates/crosspack-cli/src/render.rs`:

- `TerminalRenderer` owns status lines, sections, detail rows, rich table rows, and progress creation.
- `TerminalProgress` wraps `indicatif::ProgressBar` and exposes small methods such as `set`, `set_message`, `print_status`, `print_line`, `finish_success`, and `finish_abandon`.
- Command flows should not write raw escape sequences.
- Command flows should not create independent progress implementations.

### Install Progress Shape

Install should model phases as messages on a single progress bar or spinner:

- `preflight`
- `download`
- `verify`
- `install`
- `expose`
- `receipt`
- `complete`

Known-total download progress can update byte position/length when practical. Unknown-total downloads should use a spinner or indeterminate style rather than hand-built moving bars.

### PTY Test Strategy

Add tests at three levels:

- Unit tests for pure renderers and policies.
- Command-level tests for stdout/stderr separation in non-PTY execution.
- PTY tests for interactive behavior, especially that progress redraws do not leak into final output snapshots.

Tests should run focused by default in `crosspack-cli`; if a PTY dev-dependency is slow or platform-sensitive, gate it behind a feature or keep it limited to Linux/macOS CI.

## Success Criteria

- No raw install progress escape sequences are authored in Crosspack code outside third-party libraries.
- `cargo test -p crosspack-cli` covers install progress policy and renderer behavior.
- Redirected stdout contains no spinner frames or ANSI cursor clear sequences.
- PTY output for a representative command has no duplicated stale progress lines.
- Plain output contract tests remain green.
- `cargo fmt --all --check`, `cargo clippy -p crosspack-cli --all-targets -- -D warnings`, and `cargo test -p crosspack-cli` pass.

## Risks And Mitigations

- Risk: plain output changes accidentally. Mitigation: write regression tests before touching formatters.
- Risk: progress disappears in common terminals. Mitigation: make progress policy independently testable and default to stderr TTY detection.
- Risk: dependency sprawl. Mitigation: start with existing `indicatif`; add direct `console` in Phase 2, add `assert_cmd` for captured-output tests, and add `rexpect` for one Unix-only PTY regression.
- Risk: over-polishing rich output. Mitigation: keep Phase 1 narrowly focused on progress; Phase 2 only changes rich output through explicit formatters.

## Decisions

- Use `assert_cmd` for captured-output regression tests.
- Use `rexpect` for one narrow Unix-only PTY regression that exercises redraw behavior beyond captured stdout/stderr checks.
- Keep progress policy internal in this PR. Revisit a public `--progress auto|always|never` flag after the internal policy has real usage evidence.
- Defer public `--color` unless Phase 2 styling work needs explicit color forcing; honor `NO_COLOR` internally when color policy is introduced.
- Successful install progress should clear fully and rely on existing install status/detail lines for durable output.
- Implement Phase 1 and Phase 2 in the same PR, but complete and verify Phase 1 before starting Phase 2.
