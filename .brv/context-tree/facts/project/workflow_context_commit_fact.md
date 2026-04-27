---
title: Workflow Context Commit Fact
summary: Workflow-only PRs now run a lightweight workflow lint check via actionlint, while docs paths were removed from expensive Rust CI and release filters.
tags: []
related: []
keywords: []
createdAt: '2026-04-27T09:52:07.448Z'
updatedAt: '2026-04-27T09:52:07.448Z'
---
## Reason
Capture lasting workflow-filtering and validation changes from the session

## Raw Concept
**Task:**
Record workflow validation and filter tightening decisions from the session

**Changes:**
- Added a Workflow Lint workflow for .github/workflows/**
- Kept workflow-only changes out of the expensive Rust CI matrix
- Removed docs paths from Rust CI, Release Please, and prerelease artifact path filters
- Recorded new commits 1bc8803 and 629b980
- Updated PR #105

**Files:**
- .github/workflows/workflow-lint.yml
- README.md
- docs/architecture.md

**Flow:**
workflow-only change -> actionlint via Workflow Lint -> expensive Rust CI skipped -> docs path filters narrowed -> PR updated

**Timestamp:** 2026-04-27

**Author:** assistant

## Narrative
### Structure
The session narrowed expensive/release path filters to code and packaging inputs, while adding lightweight validation for workflow-file changes.

### Dependencies
Relies on actionlint for workflow-file validation and GitHub Actions path filters for CI/release scoping.

### Highlights
This fixed the gap where workflow-only PRs would have skipped the only actionlint coverage, and it reduced noisy docs-triggered expensive jobs.

### Rules
Workflow-only PRs must still be validated, but they should not trigger the full Rust matrix.

### Examples
Example validation sequence: git diff --check, then actionlint on changed workflows.

## Facts
- **workflow_only_pr_validation**: Workflow-only PRs now run a lightweight Workflow Lint workflow that executes actionlint. [project]
- **workflow_ci_scope**: Workflow-only changes remain out of the expensive Rust CI matrix. [project]
- **docs_path_filters**: README.md and docs/architecture.md were removed from Rust CI, Release Please, and prerelease artifact path filters. [project]
- **context_doc_commit**: A new .brv context doc was added in a separate docs commit. [project]
- **pull_request_number**: The PR updated was #105. [project]
- **validation_steps**: Validation run included git diff --check and actionlint on the changed workflows. [project]
- **commit_ids**: New commits were 1bc8803 and 629b980. [project]
