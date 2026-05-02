---
title: Pending uncommitted changes review outcome 2026-04-29
summary: Review found five issues in identity install/uninstall and state handling; verification scope covered the pending Rust diff without edits or tests.
tags: []
related: [facts/project/pending_change_review_request.md, facts/project/pending_uncommitted_changes_review_outcome_2026_04_29.overview.md, facts/project/commit_and_pr_outcome_for_installed_identity_profile_model.md, facts/project/commit_and_pr_outcome_for_installed_identity_profile_model.overview.md, facts/project/pr_112_review_fix_outcome.md, facts/project/pr_112_review_fix_outcome.overview.md, facts/project/pending_change_review_request.overview.md, facts/project/installed_state_and_rollback_regression_risk.overview.md, facts/project/review_fix_verification_for_high_leverage_rework.overview.md]
keywords: []
createdAt: '2026-04-29T19:34:27.567Z'
updatedAt: '2026-04-29T19:34:27.567Z'
---
## Reason
Record actionable findings from review of pending uncommitted changes

## Raw Concept
**Task:**
Review pending uncommitted changes in /home/ianpascoe/code/crosspack and capture durable findings

**Changes:**
- Identified five actionable findings in identity uninstall, resolution, service lookup, exposure cleanup, and state mixing
- Captured the exact verification scope used for the review

**Files:**
- crates/crosspack-cli/src/command_flows.rs
- crates/crosspack-installer/src/uninstall.rs
- crates/crosspack-cli/src/dispatch.rs
- crates/crosspack-installer/src/layout.rs
- crates/crosspack-installer/src/pins.rs
- crates/crosspack-cli/src/core_flows.rs
- crates/crosspack-installer/src/receipts.rs

**Flow:**
review pending diff -> identify impacted flows -> trace file/line references -> record findings and verification scope

**Timestamp:** 2026-04-29T19:34:07.689Z

**Author:** assistant

## Narrative
### Structure
This review outcome summarizes a pending-diff code review focused on identity-based install and uninstall behavior, resolution, service state, and exposure cleanup.

### Dependencies
Findings depend on the interaction between CLI command flows, installer uninstall logic, pin loading, receipt/service state readers, and exposed binary/completion cleanup.

### Highlights
The review reported five actionable issues and explicitly noted that the scope was limited to the pending uncommitted Rust diff without edits or tests.

## Facts
- **identity_uninstall_dependency_safety**: A review of the pending uncommitted changes found that identity uninstall bypasses dependency safety and can remove a selected dependency identity while it is still reachable from an installed root. [project]
- **identity_scoped_pins_resolution_gap**: Identity-scoped pins are written but not applied by resolution because resolution still loads pins as package_name to requirement from file stems and passes them directly to the resolver. [project]
- **identity_service_state_mismatch**: Service commands do not see services written for identity installs because service state is keyed by receipt.name while identity install writes identity-keyed service files. [project]
- **identity_uninstall_global_exposure_risk**: Exposed binaries and completions remain global, but identity uninstall removes recorded exposed bins and completions unconditionally, which can break another identity for the same package. [project]
- **identity_legacy_state_mixing**: Identity state is mixed with legacy package-name sidecars during install, which can leave stale or duplicate sidecar state and make behavior depend on which state reader is used. [project]
- **review_verification_scope**: The verification scope for the review covered the pending uncommitted Rust diff and relevant touched files, with no file edits and no tests run. [project]
