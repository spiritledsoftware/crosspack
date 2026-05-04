---
consolidated_at: '2026-05-03T02:26:31.664Z'
consolidated_from:
  - {date: '2026-05-03T02:26:31.664Z', path: facts/project/transaction_recovery_hardening_review_outcome_2026_05_02.abstract.md, reason: 'These three files describe the same transaction recovery hardening code-quality review outcome, with the abstract and overview duplicating the canonical markdown note. The markdown file is the richest source and should absorb the unique points from the companion summaries.'}
  - {date: '2026-05-03T02:26:31.664Z', path: facts/project/transaction_recovery_hardening_review_outcome_2026_05_02.overview.md, reason: 'These three files describe the same transaction recovery hardening code-quality review outcome, with the abstract and overview duplicating the canonical markdown note. The markdown file is the richest source and should absorb the unique points from the companion summaries.'}
---
# Transaction Recovery Hardening Review Outcome 2026-05-02

## Summary
Tasks 1-4 review remained QUALITY_ISSUES because rollback still collapses empty/corrupt active marker state to clean/no-active behavior; directory sync best-effort fix is resolved.

## Reason
Record the code quality review outcome for tasks 1-4 after active marker boundary fixes

## Raw Concept
**Task:**
Re-run code quality review for Tasks 1-4 after active marker CLI boundary fixes

**Changes:**
- Assessed Rust correctness, durability helper safety, transaction compatibility risks, test quality, and API exposure
- Found a portability issue in durable directory sync
- Found an active transaction marker recovery classification gap
- Confirmed directory sync best-effort fix
- Identified remaining CLI boundary issues for empty/corrupt active markers
- Marked the review as blocking Task 5
- Identified rollback boundary regression in crosspack-cli
- Confirmed directory sync best-effort fix appears resolved

**Files:**
- .agents/plans/2026-05-02-transaction-recovery-v0-5-inventory.md
- crates/crosspack-installer/src/durable.rs
- crates/crosspack-installer/src/lib.rs
- crates/crosspack-installer/src/tests.rs
- crates/crosspack-installer/src/transactions.rs
- crates/crosspack-cli/src/command_flows.rs

**Flow:**
review request -> inspect boundary handling -> identify residual rollback issue -> record required fixes

**Timestamp:** 2026-05-02

**Author:** user

## Narrative
### Structure
This review outcome captures a task-level quality assessment focused on transaction recovery hardening at installer and CLI boundaries.

### Dependencies
The remaining issue depends on replacing legacy active transaction reads with marker-aware handling in rollback paths.

### Highlights
QUALITY_ISSUES remained due to rollback still treating invalid active marker state as absent/no-active, which can hide corruption.

### Rules
Route rollback through read_active_transaction_marker(). Treat Absent as the only no active marker case. Treat Invalid as repair-required/fail-closed at rollback CLI boundary.

### Examples
Required fix: add focused CLI tests for empty and corrupt active marker behavior in rollback.

## Facts
- **transaction_recovery_review_status**: The code quality review for Tasks 1-4 returned QUALITY_ISSUES after active marker CLI boundary fixes. [project]
- **rollback_active_marker_handling**: Rollback in crates/crosspack-cli/src/command_flows.rs still uses legacy read_active_transaction() when run with None, which can collapse an empty active marker to no-active behavior. [project]
- **rollback_boundary_requirement**: Rollback should route through read_active_transaction_marker() and treat Absent as the only no-active case while treating Invalid as repair-required/fail-closed. [project]
- **directory_sync_status**: The previously flagged directory sync issue appears resolved because crates/crosspack-installer/src/durable.rs makes directory sync best-effort. [project]

## Consolidated overview
- The abstract and overview are summaries of the same review outcome and should be absorbed into this canonical file.
- Key preserved points: QUALITY_ISSUES remains, rollback boundary handling is still wrong, Task 5 is blocked, and directory sync is resolved.