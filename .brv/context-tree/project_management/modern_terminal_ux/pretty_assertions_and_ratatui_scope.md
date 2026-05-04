---
title: Pretty Assertions and Ratatui Scope
summary: The plan adds pretty_assertions as selective dev tooling for output-heavy assertions and keeps ratatui out of scope for this CLI-focused pass.
tags: []
related: [project_management/modern_terminal_ux/modern_terminal_ux_planning_pause.md, project_management/modern_terminal_ux/insta_snapshot_harness_task.md, project_management/modern_terminal_ux/spec_placement_update.md, project_management/modern_terminal_ux/spec_compliance_review_request.md]
keywords: []
createdAt: '2026-05-03T09:57:47.282Z'
updatedAt: '2026-05-03T09:57:47.282Z'
---
## Reason
Capture the planning decision about test assertions and CLI scope.

## Raw Concept
**Task:**
Document a planning decision for the modern terminal UX work.

**Changes:**
- Added pretty_assertions to the dev-tooling harness
- Defined selective usage guidance for output-heavy assertions
- Excluded ratatui from the current CLI-focused pass

**Flow:**
planning decision -> update spec/plan -> constrain scope for current pass

**Timestamp:** 2026-05-03

**Author:** Ian

## Narrative
### Structure
This decision belongs with the modern terminal UX planning notes and affects test tooling scope.

### Dependencies
Applies to the dev-tooling harness used for output-heavy assertions in the current CLI-focused work.

### Highlights
The work intentionally stays CLI-focused and avoids introducing ratatui at this stage.

### Rules
Use pretty_assertions selectively for output-heavy assertions. Do not include ratatui in this pass.

## Facts
- **pretty_assertions_dependency**: pretty_assertions = "1.4" was added as planned dev tooling. [project]
- **pretty_assertions_usage**: pretty_assertions should be used selectively for output-heavy assertions. [project]
- **ratatui_scope**: ratatui is explicitly out of scope for this CLI-focused pass. [project]
