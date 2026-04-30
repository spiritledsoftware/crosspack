# Transaction Recovery v0.5 Hardening Spec

**Status:** roadmap, non-GA
**Related shipped docs:** `docs/transaction-rollback-spec.md`, `docs/install-flow.md`
**Last updated:** 2026-04-29

## Problem

Crosspack has typed transaction status, coordinator-owned begin/status cleanup APIs, rollback snapshots, repair, and doctor integration. The next hardening step is durability: transaction records should survive crashes predictably, rollback payload coverage should be complete, and recovery policy should be explicit enough to trust under interrupted installs, upgrades, and uninstalls.

## Goals

- Make transaction journal and metadata writes crash-durable.
- Ensure rollback payload coverage matches all lifecycle side effects.
- Make recovery and repair decisions deterministic for every transaction status.
- Keep active transaction checks centralized in installer/CLI boundaries.
- Preserve existing lifecycle output contracts unless explicitly extended.

## Non-Goals

- No background daemon.
- No distributed locking across machines.
- No filesystem snapshot dependency.
- No interactive repair wizard.

## Current State

- Transaction state lives under `state/transactions/`.
- Current status values are typed: `planning`, `applying`, `committed`, `rolling_back`, `rolled_back`, `failed`.
- Rollback/repair replay package mutating journal steps in reverse order.
- Snapshot payloads cover package trees, receipts, binaries, completions, GUI assets, and native sidecar state.
- Staging directories exist but are not yet a complete durability story.

## Target Behavior

Transaction lifecycle:

1. Resolve and preflight the full plan.
2. Create metadata with `planning` status.
3. Persist rollback payloads for planned mutable steps.
4. Acquire active marker.
5. Transition to `applying`.
6. Execute steps with durable journal entries after each completed step.
7. Transition to `committed`.
8. Clear active marker and remove staging.

Recovery behavior:

- `planning`: safe cleanup or rollback depending on staged payload presence.
- `applying`: rollback completed steps.
- `committed`: finalize cleanup and clear stale active marker.
- `rolling_back`: resume rollback.
- `rolled_back`: clear stale active marker.
- `failed`: block mutation and direct user to repair.

## Architecture

The transaction coordinator owns state transitions. CLI command flows should not write transaction files directly except through coordinator APIs.

```text
CLI mutating command
        |
        v
transaction coordinator
        |
        +--> metadata/status document
        +--> active marker
        +--> journal writer
        +--> staging payloads
```

Installer APIs own file durability helpers. CLI owns recovery UX and command routing.

## Data/State Model

Transaction metadata:

- `version`
- `txid`
- `operation`
- `status`
- `started_at_unix`
- `snapshot_id`
- optional future `plan_digest`

Journal entries:

- `seq`
- `step`
- `state`
- `package`
- `path`
- `rollback_payload_ref`

Durability requirements:

- Metadata writes are atomic.
- Journal append is flushed before the corresponding mutation is considered durable.
- Staging payloads are written before forward mutation.
- Directory entries are synced where supported by platform APIs.

## CLI/UX Contracts

Existing recovery messages remain stable where possible:

```text
transaction: clean
transaction: active <txid>
transaction: failed <txid>
rolled back <txid>
rollback failed <txid>
```

New diagnostic detail can be additive:

```text
transaction_detail txid=<txid> status=applying operation=upgrade step=install_package:ripgrep
```

Plain output must remain script-friendly and deterministic.

## Failure Modes

- Metadata unreadable: fail closed for mutations, `doctor` reports repair requirement.
- Journal corrupt: fail closed; repair can quarantine only if no active mutation is implied.
- Missing rollback payload: fail rollback and mark transaction `failed`.
- Rollback action fails: preserve primary error and mark transaction `failed`.
- Active marker without metadata: block mutation and report repair guidance.

## Testing Requirements

- Crash simulation after metadata write but before active marker.
- Crash simulation after active marker but before apply.
- Crash simulation after each journaled package mutation type.
- Rollback idempotence by running repair/rollback twice.
- Journal corruption tests.
- Missing payload tests.
- Native uninstall replay ordering tests.
- Snapshot id consistency tests across multi-package transactions.

## Rollout Plan

1. Inventory every mutating file operation and rollback payload requirement.
2. Introduce durable journal writer with platform-aware sync behavior.
3. Route all transaction state writes through coordinator APIs.
4. Add crash simulation tests using controlled failure hooks.
5. Tighten `doctor`, `repair`, and `rollback` diagnostics.

## Open Questions

- Should `plan_digest` be required before v0.5 is considered complete?
- How much fsync behavior should be best-effort on platforms with weaker directory sync support?
- Should repair ever auto-quarantine corrupt transaction state, or always require explicit user action?
