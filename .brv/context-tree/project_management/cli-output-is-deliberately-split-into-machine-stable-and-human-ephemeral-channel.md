---
confidence: 0.93
sources:
  - architecture/_index.md
  - project_management/_index.md
  - project/_index.md
synthesized_at: '2026-05-03T12:13:37.338Z'
type: synthesis
---

# CLI output is deliberately split into machine-stable and human-ephemeral channels

A recurring design choice is to keep stdout stable for automation while moving progress, rich rendering, and ephemeral feedback elsewhere. That same split appears in terminal UX architecture, snapshot-based verification, and workflow rules that preserve machine-readable output contracts while allowing human-friendly presentation.

## Evidence

- **architecture**: The terminal_interface_polish summary says Crosspack stays CLI-focused, ephemeral progress should move to stderr, indicatif should handle install progress, and stable automation should remain on stdout.
- **project_management**: The output split summary says stdout remains the stable source of machine-readable truth while ephemeral progress and richer rendering move to stderr or dedicated renderers.
- **project**: The modern terminal UX review passed tests that specifically protect render and snapshot behavior, showing the output contract is verified as part of routine validation.
