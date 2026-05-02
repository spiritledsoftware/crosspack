---
title: Fresh Checkout on Both Repos
summary: Root repo and registry submodule were force-reset to origin/main; registry is at 8be2751 and working trees are clean.
tags: []
related: [facts/project/registry_commit_and_push_outcome.md, facts/project/worktree_file_access_confirmation.md, facts/project/registry_submodule_pointer_mismatch.overview.md, facts/project/fresh_checkout_refresh_outcome.md, facts/project/fresh_checkout_refresh_outcome.overview.md, facts/project/fresh_checkout_on_both_repos.overview.md, facts/project/registry_submodule_pointer_mismatch.md]
keywords: []
createdAt: '2026-04-29T01:08:49.090Z'
updatedAt: '2026-04-29T01:08:49.090Z'
---
## Reason
Record the repository reset outcome for future reference

## Raw Concept
**Task:**
Document the force fresh checkout outcome for the root repo and registry submodule

**Changes:**
- Force-reset the root repository to origin/main
- Force-reset the registry submodule to origin/main
- Confirmed clean working trees after checkout

**Flow:**
force reset root repo -> force reset registry submodule -> verify clean working trees

**Timestamp:** 2026-04-29T01:08:44.723Z

**Author:** Ian

## Narrative
### Structure
This note captures the final state after a requested fresh checkout affecting both the root repository and its registry submodule.

### Dependencies
The refresh action discarded dirty root .brv/plan changes before resetting the branches.

### Highlights
The registry submodule ended at 8be2751 with no submodule pointer mismatch, and both repos were left clean.

## Facts
- **root_repo_branch_state**: The root repo was reset to origin/main. [project]
- **registry_submodule_branch_state**: The registry submodule was reset to origin/main. [project]
- **registry_submodule_commit**: The registry submodule is at commit 8be2751. [project]
- **working_tree_state**: The working trees are clean after the fresh checkout. [project]
