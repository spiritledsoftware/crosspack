# Modern Terminal UX Spec

## Goal

Make Crosspack rich terminal output feel like a polished modern package manager while preserving plain stdout contracts for scripts.

## Direction

Crosspack remains a CLI, not a full-screen TUI. Rich output should be restrained, readable, and stable in PTYs: concise status glyphs, calm color, aligned summaries, and progress that does not jitter. Plain output remains the compatibility surface.

## Scope

Included:

- Add `insta` snapshot coverage for rich terminal rendering before changing the visual language.
- Add `pretty_assertions` for readable diffs in output-heavy unit tests.
- Add internal/test-only UI capture hooks so snapshots can be generated deterministically without relying exclusively on real PTYs.
- Replace ASCII rich badges such as `[OK]` and `[..]` with package-manager-style status glyphs in rich mode.
- Keep color/style centralized in `crates/crosspack-cli/src/render.rs`.
- Make rich section headers and detail rows quieter and more structured.
- Improve progress templates so labels do not resize, bars do not dominate, and install phase messages stay readable.
- Polish rich output for existing hotspots that already have formatter seams: `search`, `info`, install outcomes, empty states, registry status lines, update lines, and compact tables.
- Keep dry-run transaction preview lines and other machine contracts unchanged.

Excluded:

- No public `--color` or `--progress` flags in this pass.
- No documented public `--snapshot` or `--dump-ui-state` CLI surface.
- No full-screen `ratatui` or keyboard interaction.
- No resolver, registry trust, installer state, or transaction behavior changes.

## Output Contracts

Plain output must remain deterministic. Do not change line shapes for:

- `transaction_preview`
- `transaction_summary`
- `risk_flags`
- `change_add`, `change_remove`, `change_replace`, `change_transition`
- `update summary: updated=<n> up-to-date=<n> failed=<n>`
- registry add/remove/list machine fields in plain mode
- shell snippets and generated completion payloads

## Rich Visual Language

Rich status markers:

- `ok`: `✓`
- `warn`: `!`
- `error`: `×`
- `step`: `•`
- unknown: `•`

These markers are rich-mode decoration only. Tests should assert uncolored text; color is additive and may be stripped in captured output.

Sections should render as simple titles, not banner syntax. Prefer `Installed ripgrep 14.1.0` over `== Installed ripgrep 14.1.0 ==`.

Detail rows should align keys without table chrome. Prefer `  archive       tar.zst` over `STEP | archive: | tar.zst`.

## Progress Shape

Progress should render only when the existing policy enables rich stderr progress. Use one stable template for determinate work:

```text
{spinner} {prefix:<10} {wide_msg} {bar:16} {pos}/{len} {elapsed_precise}
```

Rules:

- Prefix is stable (`install`, `upgrade`, `update`, `uninstall`, `self-update`).
- Message contains changing context (`ripgrep download 2/7 1.2 MB/4.8 MB`).
- Width-heavy details go in `{wide_msg}` so the bar does not push important text offscreen.
- Successful progress clears and durable result lines carry the final summary.

## Snapshot Harness

Before the visual pass, add a dev-only `insta` layer so terminal output can be inspected as artifacts, not inferred from unit assertions alone.

Use `insta = "1.47"` as a `crosspack-cli` dev-dependency. Start with string snapshots via `insta::assert_snapshot!` for deterministic renderer output:

- rich status line gallery
- rich empty states
- rich compact tables, including Unicode display-width cases
- rich install outcome details
- normalized captured command output for representative commands where fixture setup is cheap

Snapshot tests should normalize dynamic data before assertion:

- absolute temp paths become `[PREFIX]`
- elapsed times become `[ELAPSED]`
- snapshot IDs, txids, and hashes become `[ID]` where present
- ANSI escape sequences are stripped unless the test explicitly verifies styling

Keep PTY tests narrow. PTY coverage should prove no raw redraw noise leaks. `insta` coverage should carry most of the visual review burden because snapshots are easier to review, update, and diff.

Developer workflow:

```bash
cargo test -p crosspack-cli terminal_snapshot -- --test-threads=1
cargo insta review --workspace
```

CI should run snapshot tests without automatically accepting changes. Developers update snapshots intentionally after reviewing them.

Use `pretty_assertions = "1.4"` as a companion dev-dependency for non-snapshot formatter tests. It should be imported selectively in output-heavy test scopes where diff readability matters, not applied as a repo-wide rewrite.

## Internal Capture Hooks

Add a deterministic capture path for development and tests, but keep it internal. Prefer hidden flags or environment-gated behavior over a public command contract.

Recommended shape:

- `CROSSPACK_INTERNAL_UI_SNAPSHOT=1`: force deterministic rich output decisions for test fixtures.
- `CROSSPACK_INTERNAL_TERM_WIDTH=<cols>`: pin terminal width for snapshots.
- `CROSSPACK_INTERNAL_NO_COLOR=1`: strip color while preserving rich layout/glyph decisions.
- Hidden `--dump-ui-state` only if environment variables are insufficient for debugging renderer decisions.

Avoid a public `--snapshot` flag. Snapshotting is a development workflow, not user functionality, and public flags create support and compatibility expectations.

What to dump if `--dump-ui-state` becomes necessary:

- resolved `OutputStyle`
- resolved progress policy
- resolved color policy
- terminal width source
- stdout/stderr TTY booleans
- whether PTY/captured mode was detected

The dump should be stable, plain text, and test-only documented under `.agents/specs`, not advertised in README/help output.

## Ratatui Decision

Do not switch this pass to `ratatui`.

`ratatui` is the right tool when Crosspack needs a full-screen package browser, keyboard navigation, alternate-screen rendering, panels, tabs, or live dashboard state. This pass needs polished command output, stable stderr progress, and reviewable terminal snapshots. The lower-risk stack is `indicatif`, `console`, `anstyle`, `insta`, and `pretty_assertions`.

## Testing

Add focused unit tests for pure formatters first:

- `insta` snapshots for terminal output galleries before visual changes
- `pretty_assertions` for output-heavy non-snapshot assertions
- internal deterministic capture hooks for fixture generation
- rich status markers use modern glyphs and plain mode is unchanged
- rich section headers no longer use ASCII banners
- rich install detail rows are badge-free and pipe-free
- progress completion line is suppressed or quiet enough not to duplicate install outcomes
- compact table rich output remains display-width aware

Then run:

- `cargo test -p crosspack-cli render_status_line -- --test-threads=1`
- `cargo test -p crosspack-cli render_ -- --test-threads=1`
- `cargo clippy -p crosspack-cli --all-targets -- -D warnings`

## Risks

- Rich output snapshot expectations may need coordinated test updates. Mitigation: update only rich-mode expectations, not plain contract assertions.
- Unicode glyphs could be undesirable in some terminals. Mitigation: glyphs are only rich output; plain remains ASCII and stable.
- Progress could regress under redirection. Mitigation: preserve existing progress policy and captured-output tests.
