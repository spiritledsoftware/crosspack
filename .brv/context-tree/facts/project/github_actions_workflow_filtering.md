---
title: GitHub Actions Workflow Filtering
summary: Workflow filters now use allow-list paths for product-impacting files, registry submodule bumps are treated as non-release changes, Release Please hides non-release changelog sections, and validation passed.
tags: []
related: [facts/project/release_please_sections_visible.md, facts/project/workflow_context_commit_fact.md, facts/project/github_actions_workflow_filtering.overview.md, facts/project/release_please_sections_visible.overview.md, facts/project/workflow_context_commit_fact.abstract.md]
keywords: []
createdAt: '2026-04-27T09:41:25.858Z'
updatedAt: '2026-04-27T09:41:25.858Z'
---
## Reason
Capture durable workflow gating decisions and validation outcomes for release and CI behavior.

## Raw Concept
**Task:**
Document the repo-wide GitHub Actions workflow filtering and release gating decisions.

**Changes:**
- Switched CI to allow-list paths filters for product-impacting files
- Scoped Release Please to product-impacting main pushes
- Scoped prerelease artifacts to product-impacting release branch pushes
- Restricted dependency review to Cargo manifest changes
- Hidden non-release changelog sections in Release Please
- Preserved the Homebrew bump-formula-pr fix

**Files:**
- .github/workflows/ci.yml
- .github/workflows/release-please.yml
- .github/workflows/prerelease-artifacts.yml
- .github/workflows/dependency-review.yml
- .release-please-config.json

**Flow:**
product-impacting file change -> workflow paths filter matches -> CI/release jobs run; non-product or registry-only changes -> expensive jobs skipped

**Timestamp:** 2026-04-27

**Author:** assistant

## Narrative
### Structure
The workflow policy now separates product-impacting changes from registry-only and workflow-only changes using allow-list path filters. Release Please is additionally narrowed by changelog section visibility so release notes only surface feat, fix, and perf work.

### Dependencies
This policy depends on GitHub Actions path filters, Release Please configuration, and validation tooling such as actionlint and git diff --check.

### Highlights
The key decision is that registry submodule bumps are non-release changes and should not trigger the full Rust matrix unless paired with product code changes.

### Rules
Registry submodule bumps are non-release changes; they should not create Crosspack releases or run the full Rust matrix unless combined with product code changes.

### Examples
Example: a registry-only bump skips release creation and the expensive Rust matrix; a Cargo.toml or crate source change can still trigger CI, release, and dependency review as appropriate.

## Facts
- **registry_submodule_bumps**: Registry submodule bumps should be treated as non-release changes for this repo. [project]
- **ci_filtering_strategy**: Registry-only and workflow-only PRs should skip expensive Rust jobs by default via allow-list paths filters. [project]
- **ci_paths**: CI uses allow-list paths for product-impacting files only: Cargo.toml, Cargo.lock, crates/**, scripts/**, README.md, docs/architecture.md. [project]
- **release_please_paths**: Release Please runs on main pushes only when those product-impacting paths change. [project]
- **prerelease_artifacts_paths**: Prerelease Artifacts runs on release/** pushes only when those product-impacting paths change. [project]
- **dependency_review_paths**: Dependency Review runs only when Cargo.toml or Cargo.lock changes. [project]
- **release_please_hidden_sections**: The Release Please changelog hides refactor, docs, ci, and build sections so CI/docs/build-only conventional commits do not become release-facing. [project]
- **homebrew_fix**: The prior Homebrew fix removed the redundant --version flag from brew bump-formula-pr. [project]
- **validation_results**: Validation succeeded with git diff --check, actionlint, and Node parsing of .release-please-config.json. [project]
