---
confidence: 0.88
sources:
  - project_management/_index.md
  - facts/_index.md
  - architecture/_index.md
synthesized_at: '2026-05-03T10:45:40.126Z'
type: synthesis
---

# Snapshot- and review-first workflows are used to protect visual and release behavior

Multiple domains show a consistent operational pattern: changes are validated through snapshots, review gates, and explicit workflow checks before they are treated as complete. This is not just test tooling; it is part of the repo’s broader control strategy for visual, installer, and release-sensitive behavior.

## Evidence

- **project_management**: The modern terminal UX cluster defines a sequence of planning pause, spec placement alignment, snapshot harness setup, then implementation/visual polish, with visual regression strategy defined as Task 0.
- **facts**: The project workflow summary says the repo emphasizes inspecting state, keeping .brv artifacts out of PRs, and verifying with fmt/lint/test gates before pushing or updating PRs.
- **architecture**: The architecture summary says deterministic automation and reviewable outputs are preferred, and that validation-first workflows and explicit path filtering are part of the shared typed-boundary strategy.
