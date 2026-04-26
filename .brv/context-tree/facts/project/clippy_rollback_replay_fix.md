---
title: Clippy rollback replay fix
summary: 'The rollback journal sort in crosspack-cli was changed from descending sort_by to sort_by_key with Reverse to satisfy Clippy while preserving replay order; verification passed with the default available Cargo toolchain and PR #99 was opened.'
tags: []
related: []
keywords: []
createdAt: '2026-04-26T11:08:03.344Z'
updatedAt: '2026-04-26T11:08:03.344Z'
---
## Reason
Preserve a lasting implementation detail and verification outcome from the conversation

## Raw Concept
**Task:**
Document the Clippy-driven rollback replay fix and its verification outcome

**Changes:**
- Replaced descending sort_by with sort_by_key using Reverse in rollback journal ordering
- Validated formatting, Clippy, and workspace tests
- Opened PR #99

**Files:**
- crates/crosspack-cli/src/command_flows.rs

**Flow:**
reproduce lint failure -> apply minimal behavior-preserving sort change -> verify fmt/clippy/tests -> open PR

**Timestamp:** 2026-04-26

**Author:** assistant

## Narrative
### Structure
The fix was made in the crosspack-cli rollback journal path, specifically in the descending sort used for replay ordering.

### Dependencies
Verification used the default available Cargo toolchain because `rustup run stable ...` was unavailable locally.

### Highlights
The change preserves rollback replay order while satisfying the newer Clippy lint.

### Examples
Commands run: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`.

## Facts
- **clippy_failure_location**: The Clippy failure was isolated to `unnecessary_sort_by` in `crates/crosspack-cli/src/command_flows.rs`. [project]
- **rollback_sort_order**: The rollback journal sorting was descending by sequence number for replay order. [project]
- **rollback_sort_fix**: The fix replaced `sort_by` with `sort_by_key(... Reverse ...)` to preserve behavior and satisfy Clippy. [project]
- **verification_commands**: Verification passed with `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace`. [project]
- **rustup_stable_unavailable**: The `rustup run stable ...` command could not run because no stable rustup toolchain was installed locally. [environment]
- **pull_request**: A pull request was opened at https://github.com/spiritledsoftware/crosspack/pull/99. [project]
