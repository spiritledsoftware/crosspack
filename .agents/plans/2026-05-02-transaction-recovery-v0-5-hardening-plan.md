# Transaction Recovery v0.5 Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Do not implement directly from `.agents/specs/transaction-recovery-v0-5-hardening-spec.md`; this plan is the implementation boundary. Do not commit unless the user explicitly asks.

**Goal:** Make Crosspack transaction metadata, journals, rollback payloads, and recovery decisions durable and deterministic across interrupted install, upgrade, uninstall, rollback, repair, and doctor flows.

**Architecture:** Keep transaction state ownership inside `crosspack-installer`. CLI command flows may inspect and render transaction health, but all transaction file writes, status transitions, active marker handling, journal appends, staging payload writes, and recovery decisions must go through installer-owned APIs. Preserve existing plain output line shapes and add only deterministic detail lines where the spec allows.

**Tech Stack:** Rust workspace, `anyhow`, `serde`, `serde_json`, `std::fs`/`std::io`, existing `PrefixLayout`, `TransactionCoordinator`, `TransactionMetadata`, `TransactionJournalEntry`, CLI command tests, and workspace Cargo validation.

---

## Current Context

- Transaction state lives under `state/transactions/` through `PrefixLayout`.
- `TransactionStatus` supports `planning`, `applying`, legacy `completed`, `committed`, `rolling_back`, `rolled_back`, and `failed`.
- `transactions.rs` owns active marker, metadata read/write, status update, and journal append helpers.
- `transaction_coordinator.rs` owns `begin`, status transition helpers, and `clear_active`.
- Rollback snapshots already cover package trees, receipts, exposed binaries, exposed completions, GUI assets, and optional native sidecar state.
- CLI docs describe stable plain output contracts including `transaction: clean`, `transaction: active <txid>`, `transaction: failed <txid>`, `rolled back <txid>`, and `rollback failed <txid>`.

Important limitations to address:

- Metadata writes use `fs::write` and are not atomic across crashes.
- Journal appends flush file handles but do not use a shared durability policy or directory sync.
- Active marker writes flush but do not share atomic write/directory sync semantics.
- Recovery policy is not centrally encoded for every `TransactionStatus` and active-marker combination.
- Crash simulation tests are mostly behavioral status tests, not controlled interruption tests for every mutating step family.
- `plan_digest` remains optional future metadata unless implemented as a backward-compatible optional field.

## Scope

Implement the full Transaction Recovery v0.5 Hardening spec in one PR:

1. Inventory mutation surfaces and lock current behavior.
2. Add durable transaction write primitives.
3. Route metadata, active marker, journal, and staging payload writes through those primitives.
4. Encode deterministic recovery classification for every status.
5. Add journal read/parse corruption handling.
6. Add controlled crash/failure hooks for tests only where useful.
7. Fill rollback payload coverage gaps found by the inventory.
8. Tighten `rollback`, `repair`, and `doctor` diagnostics without changing existing line shapes.
9. Update shipped docs and roadmap spec completion notes.

## Non-Goals

- No background daemon.
- No distributed lock or cross-machine coordination.
- No filesystem snapshot dependency.
- No interactive repair wizard.
- No arbitrary post-install scripts.
- No CLI output contract breakage.
- No registry trust/signature policy changes in installer code.
- No root/admin escalation behavior changes.

## Task 1: Inventory Mutating Operations And Existing Rollback Payloads

**Files:**
- Create: `.agents/plans/2026-05-02-transaction-recovery-v0-5-inventory.md`
- Read: installer and CLI transaction/lifecycle files.

- [x] Create the inventory document with a mutation coverage matrix and status policy matrix.
- [x] Include rows for package tree install/remove, receipts, binaries, completions, GUI assets, native sidecar state, cache pruning, metadata, journal, and active marker.
- [x] Search transaction and journal step names and add any source-build, bundle, or identity-specific rows found.
- [x] Do not proceed until every gap has an owner and test target.

## Task 2: Lock Current Transaction File Format Compatibility

**Files:**
- Modify: `crates/crosspack-installer/src/tests.rs`
- Modify only if exposed for tests: `crates/crosspack-installer/src/transactions.rs`

- [x] Add metadata round-trip tests with and without `snapshot_id`.
- [x] Add a legacy metadata parse compatibility test.
- [x] Run: `cargo test -p crosspack-installer transaction_metadata_round_trips legacy_transaction_metadata_still_parses -- --test-threads=1`.

## Task 3: Add Durable Filesystem Write Primitives

**Files:**
- Create: `crates/crosspack-installer/src/durable.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Modify: `crates/crosspack-installer/src/tests.rs`

- [x] Add crate-private helpers: `write_file_atomic`, `append_line`, `remove_file_if_exists_durable`, and `sync_directory`.
- [x] Use sibling temp files, `sync_all`, atomic rename, and best-effort parent directory sync.
- [x] Add tests for replacement, append, idempotent remove, and missing-directory sync tolerance.
- [x] Run: `cargo test -p crosspack-installer durable_ -- --test-threads=1`.

## Task 4: Route Transaction Metadata, Journal, And Active Marker Through Durable Helpers

**Files:**
- Modify: `crates/crosspack-installer/src/transactions.rs`
- Modify: `crates/crosspack-installer/src/transaction_coordinator.rs`
- Modify: `crates/crosspack-installer/src/tests.rs`

- [x] Replace metadata writes with `durable::write_file_atomic`.
- [x] Replace journal append logic with `durable::append_line` while preserving line shape.
- [x] Replace active marker removal with durable removal.
- [x] Preserve active marker conflict error shape.
- [x] Add tests for atomic metadata replacement, journal shape, marker conflict, and idempotent clear.
- [x] Run focused transaction tests.

## Task 5: Centralize Recovery Classification

**Files:**
- Modify: `crates/crosspack-installer/src/types.rs`
- Modify: `crates/crosspack-installer/src/transaction_coordinator.rs`
- Modify: `crates/crosspack-installer/src/transactions.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Modify: `crates/crosspack-installer/src/tests.rs`

- [x] Add `TransactionRecoveryAction` and `TransactionRepairReason` types.
- [x] Add `TransactionCoordinator::classify_recovery(&self) -> Result<TransactionRecoveryAction>`.
- [x] Cover `planning`, `applying`, `committed`, `completed`, `rolling_back`, `rolled_back`, and `failed`.
- [x] Fail closed for active marker without metadata, unreadable metadata, unreadable journal, and applying metadata without active marker.
- [x] Add matrix tests for every classification.

## Task 6: Add Journal Read/Parse And Corruption Handling

**Files:**
- Modify: `crates/crosspack-installer/src/types.rs`
- Modify: `crates/crosspack-installer/src/transactions.rs`
- Modify: `crates/crosspack-installer/src/tests.rs`

- [x] Add `read_transaction_journal_entries(layout, txid)`.
- [x] Accept existing line format and absent journal as empty.
- [x] Reject corrupt non-empty lines with parse context.
- [x] Add optional `package` and `rollback_payload_ref` only if needed, preserving old line compatibility.

## Task 7: Add Test-Only Crash Injection Hooks At Transaction Step Boundaries

**Files:**
- Modify installer transaction/lifecycle files as needed.
- Modify: `crates/crosspack-installer/src/tests.rs`

- [x] Add `#[cfg(test)]` crash hook types only if needed by tests.
- [x] Cover begin ordering after metadata write and after active marker.
- [x] Cover before mutation, after mutation before journal, and after journal where existing seams make this practical.
- [x] Ensure no production API exposes crash hooks.

Task 7 completion note: the only practical low-risk crash hook seam is transaction begin. `TransactionBeginCrashHook` and `begin_with_crash_hook_for_test` are `#[cfg(test)]` and cover interruption after metadata write and after active marker creation. Broader mutation-step hooks were intentionally not added because install, uninstall, exposure, native cleanup, and CLI transaction orchestration are split across multiple existing seams; threading test-only hooks through those layers would expose non-production control paths and overbuild the hardening work. Task 8 instead locks the payload-before-forward-mutation and rollback replay invariants with behavior tests at the transaction journal/snapshot boundary.

## Task 8: Complete Rollback Payload Coverage For All Lifecycle Side Effects

**Files:**
- Modify installer artifact/exposure/native/uninstall/transaction files as needed.
- Modify inventory document.

- [x] Add failing tests for any inventory gap first.
- [x] Ensure payloads are written before forward mutation.
- [x] Ensure native rollback actions run before managed snapshot restore for native steps.
- [x] Mark inventory gaps covered by test name.

Task 8 completion note: inventory gaps are closed with exact test names in `.agents/plans/2026-05-02-transaction-recovery-v0-5-inventory.md`. Cache pruning and empty-directory pruning remain explicit best-effort/non-rollback-payload policies; existing safety/reference tests cover those decisions.

## Task 9: Implement Recovery Execution For Classified Actions

**Files:**
- Modify: `crates/crosspack-installer/src/transaction_coordinator.rs`
- Modify: `crates/crosspack-installer/src/transactions.rs`
- Modify: `crates/crosspack-installer/src/tests.rs`

- [x] Add `TransactionCoordinator::repair_transaction_state(&self) -> Result<TransactionRecoveryAction>`.
- [x] Implement deterministic cleanup/finalize/clear/block behavior.
- [x] Rollback/resume rollback must be idempotent and fail closed on missing payloads.

## Task 10: Fail Closed Before Mutating Commands

**Files:**
- Modify CLI flow files and tests.

- [x] Route mutating command preflight through installer recovery classification.
- [x] Preserve existing `transaction: ...` output lines.
- [x] Add deterministic `transaction_detail` lines only as additive output.

## Task 11: Tighten Rollback, Repair, And Doctor Diagnostics

**Files:**
- Modify CLI flow/render/tests and docs.

- [x] Add deterministic reason codes for repair-required states.
- [x] Render `transaction_detail txid=<txid> status=<status> operation=<operation> step=<step-or-none>`.
- [x] Render repair actions in plain mode as `repair action=...` lines.

## Task 12: Resolve `plan_digest` And Metadata Versioning Decision

**Files:**
- Modify spec and transaction metadata code only if needed.

- [x] Default decision: `plan_digest` remains optional future metadata and is not required for v0.5 completion.
- [x] If implemented, make it optional and backward compatible.

## Task 13: Update Shipped Documentation

**Files:**
- Modify: `docs/install-flow.md`
- Modify: `docs/transaction-rollback-spec.md`
- Modify: `docs/architecture.md` only if needed.

- [x] Document atomic metadata, durable active marker, payload-before-mutation ordering, journal flush ordering, and deterministic recovery classification.

## Task 14: Validate Full Roadmap Item

- [x] Run `cargo fmt --all --check`.
- [x] Run `cargo test -p crosspack-installer -- --test-threads=1`.
- [x] Run `cargo test -p crosspack-cli -- --test-threads=1`.
- [x] Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- [x] Run `cargo build --workspace --locked`.
- [x] Run `cargo test --workspace`.
- [x] Run `scripts/validate-snapshot-flow.sh`.
- [x] Run `git diff --check`.

## Task 15: Final Spec And Plan Closeout

**Files:**
- Modify spec, plan, and inventory.

- [x] Add implementation notes to `.agents/specs/transaction-recovery-v0-5-hardening-spec.md`.
- [x] Mark completed plan tasks only after validation passes.
- [x] Ensure inventory has no unresolved gaps.

## PR Strategy

Implement this roadmap item in one PR. Complete tasks sequentially, run each focused validation before moving on, use internal review checkpoints after Tasks 4, 6, 8, 11, and 15, and do not merge until Task 14 passes.
