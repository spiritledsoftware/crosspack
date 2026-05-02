---
title: Legacy State Compatibility in Installed Package State Reader
summary: Legacy name-keyed installed package state docs no longer trigger false duplicate identity detection when an identity-keyed replacement exists; workspace verification passed.
tags: []
related: [architecture/installed_identity_profile_model/installed_identity_profile_model.md]
keywords: []
createdAt: '2026-04-29T19:31:13.172Z'
updatedAt: '2026-04-29T19:31:13.172Z'
---
## Reason
Document the final installer compatibility fix and verification results from the continued work session.

## Raw Concept
**Task:**
Document the final installer compatibility fix for legacy installed package state handling and the resulting verification status.

**Changes:**
- Tightened the compatibility exception in read_all_installed_package_states.
- Kept duplicate detection intact for genuine non-legacy duplicates.
- Completed a fresh full verification gate after the fix.

**Flow:**
seed legacy state + identity-keyed replacement -> read_all_installed_package_states -> skip false duplicate -> preserve real duplicate detection -> run workspace verification

**Timestamp:** 2026-04-29T19:31:02.666Z

**Author:** assistant

## Narrative
### Structure
This note captures the final continuation of the installer identity/profile work. It records the reader compatibility behavior, the duplicate-detection boundary, and the completed verification set.

### Dependencies
The fix depends on distinguishing legacy name-keyed state documents from identity-keyed documents during directory iteration.

### Highlights
The targeted installer suite passed, and the broader workspace gate finished clean after rerunning sequentially with a longer timeout. The pending diff remained large and focused on identity/profile implementation, tests, docs, and plan/spec updates.

### Examples
Reported verification commands included cargo test -p crosspack-installer, cargo clippy --workspace --all-targets --all-features -- -D warnings, cargo test --workspace, cargo fmt --all --check, cargo build --workspace --locked, and git diff --check.

## Facts
- **legacy_state_compatibility**: read_all_installed_package_states was fixed so legacy name-keyed state documents do not trip duplicate identity detection when an identity-keyed document for the same package also exists. [project]
- **duplicate_identity_rejection**: Real duplicate identity rejection is still preserved for non-legacy duplicate state documents. [project]
- **installer_test_verification**: Verification passed for cargo test -p crosspack-installer with 144 tests passed. [project]
- **clippy_verification**: Verification passed for cargo clippy --workspace --all-targets --all-features -- -D warnings. [project]
- **workspace_test_verification**: Verification passed for cargo test --workspace. [project]
- **format_verification**: Verification passed for cargo fmt --all --check. [project]
- **build_verification**: Verification passed for cargo build --workspace --locked. [project]
- **diff_check_verification**: Verification passed for git diff --check. [project]
