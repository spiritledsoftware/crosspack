---
title: Homebrew Tap Workflow Fix
summary: Homebrew tap sync workflow was fixed by removing a redundant --version flag from brew bump-formula-pr because the release tag URL already implied version 0.11.0.
tags: []
related: []
keywords: []
createdAt: '2026-04-27T09:30:08.468Z'
updatedAt: '2026-04-27T09:30:08.468Z'
---
## Reason
Capture the durable fix for the Homebrew tap sync workflow and its verification

## Raw Concept
**Task:**
Document the Homebrew tap workflow fix for the release synchronization workflow.

**Changes:**
- Removed --version "${{ steps.meta.outputs.version }}" from brew bump-formula-pr
- Prevented Homebrew from emitting a redundant stable version stanza
- Verified the workflow with whitespace checks and actionlint

**Files:**
- .github/workflows/homebrew-tap-sync.yml

**Flow:**
release tarball URL -> brew bump-formula-pr -> Homebrew infers version -> formula update without redundant version stanza

**Timestamp:** 2026-04-27

**Patterns:**
- `--version "${{ steps.meta.outputs.version }}"` - Removed flag from brew bump-formula-pr invocation

## Narrative
### Structure
The workflow change is minimal and isolated to the Homebrew tap sync GitHub Actions workflow.

### Dependencies
Depends on Homebrew deriving the version from the release tarball URL rather than an explicit --version argument.

### Highlights
Root cause was a redundant version stanza rejected by brew audit. Verification passed after the flag removal.

### Rules
Stable: version 0.11.0 is redundant with version scanned from URL

### Examples
The fix kept the release tarball URL and SHA unchanged while removing the explicit version argument.

## Facts
- **homebrew_bump_formula_pr_version_stanza**: The Homebrew tap sync workflow failed because brew bump-formula-pr generated a redundant version 0.11.0 stanza. [project]
- **homebrew_bump_formula_pr_version_flag**: Removing --version from brew bump-formula-pr fixed the workflow. [project]
- **homebrew_version_inference**: Homebrew can infer version 0.11.0 from the release tarball URL. [project]
- **workflow_verification**: Validation passed with git diff --check and actionlint reported no diagnostics. [project]
