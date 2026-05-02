---
title: Release Please sections visible
summary: Release Please changelog sections remain visible; suppression is handled by workflow path filters instead of hidden config flags.
tags: []
related: [facts/project/github_actions_workflow_filtering.md, facts/project/workflow_context_commit_fact.md, facts/project/github_actions_workflow_filtering.overview.md, facts/project/release_please_sections_visible.overview.md, facts/project/workflow_context_commit_fact.abstract.md]
keywords: []
createdAt: '2026-04-27T09:55:11.765Z'
updatedAt: '2026-04-27T09:55:11.765Z'
---
## Reason
Capture the decision to keep all Release Please sections visible and rely on workflow filters for suppression

## Raw Concept
**Task:**
Document the Release Please configuration decision for changelog section visibility.

**Changes:**
- Removed hidden flags from Release Please sections
- Kept workflow path gating as the release suppression mechanism

**Flow:**
workflow path filters -> suppress non-user-facing release runs; changelog sections remain visible

**Timestamp:** 2026-04-27

**Author:** assistant

## Narrative
### Structure
The release process keeps all Release Please sections visible in changelog output while workflow path filters control when release jobs run.

### Dependencies
Depends on GitHub Actions path filtering rather than hidden Release Please section configuration.

### Highlights
This preserves changelog visibility for refactor, docs, ci, build, test, and chore sections while still preventing unnecessary release runs.

## Facts
- **release_please_sections_visibility**: Release Please changelog sections should remain visible. [project]
- **release_suppression_strategy**: Release suppression should rely on workflow path filters rather than hidden changelog sections. [project]
