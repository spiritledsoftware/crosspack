---
confidence: 0.93
sources:
  - architecture/_index.md
  - project_management/_index.md
  - project/_index.md
synthesized_at: '2026-05-03T10:45:40.125Z'
type: synthesis
---

# CLI output is being split into stable machine data and ephemeral human presentation

The terminal UX work and broader architecture both enforce a boundary between stable automation output and human-facing presentation. The CLI remains the source of machine-readable truth on stdout, while progress and richer rendering move to stderr or dedicated renderers, with snapshot tooling used to protect the output contract.

## Evidence

- **architecture**: The terminal interface polish entry says Crosspack remains a CLI, not a full TUI, and that stable automation output stays on stdout while ephemeral progress moves to stderr.
- **project_management**: Modern terminal UX planning says ratatui is out of scope for this CLI-focused pass, pretty_assertions is limited to output-heavy assertions, and insta is used as a dev-only snapshot harness for rendered and PTY-normalized output.
- **project**: The task status summary notes partial PTY run evidence with insta and pretty_assertions fetched before final snapshot verification was blocked, showing output stability is treated as a testable contract.
