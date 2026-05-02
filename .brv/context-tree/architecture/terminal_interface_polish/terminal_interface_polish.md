---
title: Terminal Interface Polish
summary: Crosspack should remain a CLI with improved progress handling via indicatif, stderr-only ephemeral progress, TerminalRenderer-based human output, width-aware tables, output controls, and PTY regression tests; avoid ratatui/crossterm/dialoguer for now.
tags: []
related: []
keywords: []
createdAt: '2026-05-02T17:14:10.493Z'
updatedAt: '2026-05-02T17:14:10.493Z'
---
## Reason
Document the CLI polish recommendations for Crosspack terminal output and PTY behavior

## Raw Concept
**Task:**
Document terminal interface polish guidance for Crosspack

**Changes:**
- Recommend staying a CLI rather than becoming a full TUI
- Move ephemeral progress off stdout and onto stderr
- Refactor install progress to use indicatif instead of a hand-rolled renderer
- Add PTY regression coverage for terminal behavior

**Flow:**
progress events -> stderr progress bar -> suspend status lines -> stable stdout automation output

**Timestamp:** 2026-05-02T17:14:02.193Z

**Author:** assistant analysis

## Narrative
### Structure
The guidance centers on a CLI rendering split: ephemeral progress should be isolated from durable output, and human-facing rich output should pass through one renderer so formatting is consistent.

### Dependencies
Indicatif is already present; console, anstream/anstyle, unicode-width, snapbox/trycmd, and PTY harness tools were recommended as supporting libraries. Ratatui, crossterm, and prompt libraries were explicitly deferred.

### Highlights
The most concrete first refactor is to remove the custom install progress writer and route install through the same TerminalProgress/indicatif path used elsewhere.

### Rules
Keep stdout for stable automation lines. Put all ephemeral progress on stderr. Route all rich human output through TerminalRenderer.

### Examples
Suggested controls include --color auto|always|never and --progress auto|always|never, plus NO_COLOR and CLICOLOR_FORCE for predictable PTY and snapshot behavior.

## Facts
- **terminal_form_factor**: Keep Crosspack as a CLI with richer progress, not a full TUI. [project]
- **install_progress_renderer**: Replace the custom InstallProgressRenderer in src/main.rs with indicatif. [project]
- **progress_streams**: Write ephemeral progress on stderr and keep stdout for stable automation lines. [project]
- **human_output_renderer**: Use TerminalRenderer for rich human output instead of many direct println paths. [project]
- **table_width_measurement**: render_compact_table() currently uses cell.len(), which is byte length rather than display width. [project]
- **output_controls**: Add output controls for color and progress: --color auto|always|never, --progress auto|always|never, NO_COLOR, and CLICOLOR_FORCE. [project]
- **pty_regression_tests**: Add PTY regression tests for raw escape leaks, progress clearing, and redirected stdout readability. [project]
- **recommended_libraries**: Prefer indicatif, console, anstream/anstyle, unicode-width or console::measure_text_width, snapbox or trycmd, and PTY harness tools such as rexpect, expectrl, or portable-pty. [project]
- **avoid_library**: Avoid ratatui for the current output polish problem. [project]
- **avoid_library**: Avoid crossterm unless keyboard interaction, alternate screen, raw mode, or cursor movement is needed. [project]
- **avoid_library**: Avoid dialoguer and inquire unless prompts are added. [project]
- **cargo_run_noise**: cargo run emits multi-line progress and overwrite sequences before Crosspack starts, so compiled binary or cargo run --quiet is better for judging CLI output quality. [project]
