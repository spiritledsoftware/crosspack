---
title: Terminal Interface Polish
summary: For Crosspack terminal polish, pretty_assertions is a useful dev dependency for diffing output tests, while switching to ratatui is considered overkill for the current CLI-focused scope.
tags: []
related:
  - architecture/terminal_interface_polish/terminal_interface_polish.abstract.md
  - architecture/terminal_interface_polish/terminal_interface_polish.overview.md
keywords: []
createdAt: '2026-05-02T17:14:10.493Z'
updatedAt: '2026-05-03T09:56:00.515Z'
consolidated_at: '2026-05-03T12:13:30.914Z'
consolidated_from:
  - {date: '2026-05-03T12:13:30.914Z', path: architecture/terminal_interface_polish/terminal_interface_polish.abstract.md, reason: 'These three files describe the same terminal interface polish guidance at different levels of detail. The .md file is already the richest source, while the abstract and overview are redundant summaries that overlap heavily with it.'}
  - {date: '2026-05-03T12:13:30.914Z', path: architecture/terminal_interface_polish/terminal_interface_polish.overview.md, reason: 'These three files describe the same terminal interface polish guidance at different levels of detail. The .md file is already the richest source, while the abstract and overview are redundant summaries that overlap heavily with it.'}
---
## Reason
Capture guidance on pretty_assertions and ratatui scope for CLI terminal polish work

## Raw Concept
**Task:**
Document terminal UI and test tooling guidance for Crosspack

**Changes:**
- Recommend staying a CLI rather than becoming a full TUI
- Move ephemeral progress off stdout and onto stderr
- Refactor install progress to use indicatif instead of a hand-rolled renderer
- Add PTY regression coverage for terminal behavior
- Recommended pretty_assertions as a supporting dev tool for output-heavy tests
- Advised against switching to ratatui for the current CLI-focused work
- Kept the terminal polish strategy centered on CLI output rather than a full TUI

**Flow:**
output change -> test failure -> clearer diff via pretty_assertions; terminal polish need -> evaluate scope -> keep CLI stack instead of adopting ratatui

**Timestamp:** 2026-05-03T09:55:52.785Z

**Author:** assistant analysis

## Narrative
### Structure
The guidance splits test support into snapshot testing with insta and diff-friendly assertions with pretty_assertions, while treating ratatui as a separate, larger architectural choice.

### Dependencies
This recommendation depends on preserving Crosspack as a CLI and on keeping the test surface small for PTY and terminal-output behavior.

### Highlights
pretty_assertions improves reviewability of terminal output regressions; ratatui is unnecessary unless the project moves toward a full-screen interactive package browser or similar TUI.

### Rules
Keep stdout for stable automation lines. Put all ephemeral progress on stderr. Route all rich human output through TerminalRenderer.

### Examples
Use pretty_assertions::assert_eq selectively in output-heavy tests, but continue using insta for visual regression coverage.

## Facts
- **pretty_assertions_role**: pretty_assertions is recommended as a supporting dev tool, not a replacement for insta. [project]
- **pretty_assertions_benefit**: pretty_assertions helps inspect assertion failures for string, vector, and formatter output tests. [project]
- **pretty_assertions_integration**: pretty_assertions should be added selectively in crosspack-cli dev-dependencies alongside insta. [project]
- **ratatui_scope**: Switching to ratatui is considered overkill for the current terminal-output goals. [project]
- **recommended_terminal_stack**: The recommended stack for this pass is indicatif, console, anstyle, insta, and pretty_assertions. [project]

## Cross references
- architecture/terminal_interface_polish/terminal_interface_polish.abstract.md
- architecture/terminal_interface_polish/terminal_interface_polish.overview.md