---
title: Insta Snapshot Harness Task
summary: Modern terminal UX plan now includes an insta-based snapshot harness task to review rendered terminal output with cargo insta review before visual changes.
tags: []
related: [project_management/modern_terminal_ux/modern_terminal_ux_planning_pause.md, project_management/modern_terminal_ux/pretty_assertions_and_ratatui_scope.md, project_management/modern_terminal_ux/spec_placement_update.md, project_management/modern_terminal_ux/spec_compliance_review_request.md]
keywords: []
createdAt: '2026-05-03T09:54:22.436Z'
updatedAt: '2026-05-03T09:54:22.436Z'
---
## Reason
Capture the decision to add insta as a dev-only visual regression harness before TUI polish

## Raw Concept
**Task:**
Add insta-based snapshotting to the modern terminal UX plan as a prerequisite visual regression harness before implementation proceeds.

**Changes:**
- Added insta as a dev-only snapshotting layer for TUI development
- Scoped snapshots to rendered terminal output and PTY-normalized command output
- Defined cargo insta review as the review workflow before visual changes

**Files:**
- .agents/plans/2026-05-03-modern-terminal-ux-spec.md
- .agents/plans/2026-05-03-modern-terminal-ux-implementation-plan.md

**Flow:**
add insta dev dependency -> snapshot status/empty/table/install-output galleries -> normalize dynamic data -> review diffs with cargo insta review --workspace -> proceed to visual polish

**Timestamp:** 2026-05-03

**Author:** Ian

**Patterns:**
- `cargo insta review --workspace` - Command used to review snapshot diffs before TUI visual changes
- `insta = "1.47"` - Pinned dev dependency version for the snapshot harness

## Narrative
### Structure
The modern terminal UX plan now treats insta as Task 0, sitting ahead of implementation work so developers can inspect rendered terminal output through snapshots.

### Dependencies
This is a dev-only harness prerequisite and does not change runtime UX yet; it depends on snapshot normalization and cargo insta review tooling.

### Highlights
The plan explicitly calls for snapshot galleries, normalized dynamic data, and visual diff review before any visual polish work begins.

### Rules
Task 0: add dev dependency, snapshot rich status/empty/table/install-output galleries, normalize dynamic output, review with cargo insta review --workspace, then do visual changes.

### Examples
Examples of snapshot targets include rich status screens, empty states, tables, and install-output galleries.

## Facts
- **insta_snapshot_harness**: The TUI work should add insta for snapshotting during development before proceeding with visual polish. [project]
- **snapshot_harness_scope**: The snapshot harness is dev-only and targets rendered terminal output rather than changing runtime UX. [project]
- **snapshot_review_command**: The workflow should include cargo insta review for reviewing snapshot diffs. [project]
