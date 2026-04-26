# Terminal Interface Polish Design

## Goal

Improve Crosspack's interactive terminal experience while preserving the plain output contracts used by scripts and tests.

The first pass should make human-facing TTY output easier to scan, more consistent, and more confident without changing command behavior or machine-oriented line shapes.

## Scope

Included:

- Rich TTY-only presentation improvements for common read and lifecycle commands.
- Reusable renderer helpers for sections, status rows, key/value details, compact tables, empty states, and summaries.
- Tests that prove plain output stays stable while rich output gains structure.

Not included:

- A full-screen TUI dashboard.
- Interactive prompts or guided workflows.
- Changes to resolution, install, registry, transaction, receipt, or prefix behavior.
- Changes to dry-run machine contracts.

## Output Contract

Crosspack keeps the existing split:

- `OutputStyle::Plain` remains the automation contract.
- `OutputStyle::Rich` may add decoration, spacing, sectioning, color, and progress polish for interactive terminals.

Do not change these plain/non-interactive line shapes in this work:

- `transaction_preview`
- `transaction_summary`
- `risk_flags`
- `change_add`
- `change_remove`
- `change_replace`
- `change_transition`
- `update summary: updated=<n> up-to-date=<n> failed=<n>`
- Registry add/list/remove machine-oriented fields

## Recommended Approach

Use a rich renderer pass, not a command redesign.

Extend `TerminalRenderer` in `crates/crosspack-cli/src/render.rs` with small reusable helpers. Commands should keep producing the same underlying data and only choose richer formatting when `current_output_style()` resolves to `Rich`.

The renderer should stay intentionally modest:

- ASCII-compatible structure where practical.
- Color as emphasis, not the only information channel.
- No new runtime dependencies unless a clear need appears during implementation.
- No hidden state or command-specific behavior inside the renderer.

## Target Commands

Prioritize high-visibility output surfaces:

- `search`: clearer result grouping and empty-state guidance.
- `info`: package/version sections with aligned metadata.
- `list`: readable installed package table in rich mode.
- `outdated`: rich table with installed/latest/source columns.
- `registry list`: source rows with snapshot/trust state details.
- `doctor`: grouped path and transaction health output.
- install/upgrade/update completion summaries: more readable success/warning summaries without touching dry-run contracts.

## Renderer Shape

Add helpers that are easy to unit test and reuse:

- `render_section_header`
- status line/status row helpers for `ok`, `warn`, `error`, and `step`
- key/value detail row helper
- compact table helper with deterministic column widths
- empty-state helper that can include one actionable hint
- summary helper for completion status

Existing direct `println!` calls can remain where plain output is intentionally contract-shaped. Rich-mode formatting should be introduced only where the command already branches on `OutputStyle` or where a small formatter can preserve plain output exactly.

## Example Direction

Rich registry output can move toward this shape:

```text
== Registry Sources ==

OK   core       git    priority 100
     ready      snapshot abc123
     trusted    8f91...

WARN staging    git    priority 50
     stale      run `crosspack update`
```

Plain registry output should remain the existing stable field-oriented output.

## Testing Plan

Add focused tests rather than broad snapshot churn:

- Unit tests for new renderer helpers.
- Contract tests for plain formatters touched by the work.
- Rich-mode tests for representative commands where formatting is deterministic.
- Existing workspace verification before completion.

Verification commands:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Risks

Main risk is accidentally changing plain output while improving rich output. Mitigate this by keeping plain formatters explicit and adding regression tests before changing command output code.

Secondary risk is over-styling. Keep the first pass practical: better alignment, clearer hierarchy, better summaries, and consistent status language.
