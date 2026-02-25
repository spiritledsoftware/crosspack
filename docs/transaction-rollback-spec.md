# Transaction, Rollback, and Recovery Specification (Draft v0.5)

This document defines v0.5 transactional behavior for Crosspack installs, upgrades, and uninstalls. It adds crash recovery, rollback support, and reproducibility metadata so state transitions are safe and auditable.

**Status:** roadmap draft (non-GA). This document is a design target and does not change shipped GA guarantees until implementation is merged and released.

## Current v0.3 Recovery Contract (shipped)

Current shipped behavior already includes deterministic rollback snapshot/replay for package-level mutating steps.

Snapshot payload captured per package:

- package tree under `<prefix>/pkgs/<name>/...`
- install receipt backup for `<prefix>/state/installed/<name>.receipt`
- exposed binary entries owned by the package
- exposed package completion files
- exposed GUI assets and GUI ownership state
- native sidecar state (`<prefix>/state/installed/<name>.gui-native`)

Replay behavior:

- rollback/repair replays completed package mutating journal steps in reverse sequence (`install_package:*`, `install_native_package:*`, `upgrade_package:*`, `upgrade_native_package:*`, `uninstall_target:*`, `prune_dependency:*`),
- for native package replay, native uninstall actions run before managed snapshot restore,
- managed restore then rehydrates package tree, receipt, binaries, completions, GUI assets/state, and native sidecar state from snapshot payload.

## Scope

This spec covers:

- Transaction journaling for mutating operations.
- Automatic crash recovery and explicit rollback commands.
- Snapshot consistency binding for resolution and install receipts.
- Optional artifact signature policy enforcement.

This spec does not cover:

- Remote telemetry.
- Distributed lock coordination across hosts.
- Full filesystem snapshot integration.

## Goals

- Ensure mutating commands are recoverable after interruption.
- Guarantee idempotent rollback and deterministic replay.
- Prevent partial state from being treated as successful install state.
- Preserve existing user-facing lifecycle wording where possible.

## Non-Goals

- No requirement for copy-on-write filesystems.
- No background daemon.
- No interactive transaction conflict resolution.

## Operations Covered

Transactions are mandatory for:

- `crosspack install`
- `crosspack upgrade` (single and global)
- `crosspack uninstall`

Read-only commands (`search`, `info`, `list`, `doctor`) do not open transactions.

## Transaction State Layout

```text
<prefix>/state/transactions/
  active
  <txid>.json
  <txid>.journal
  staging/<txid>/
```

### `active`

- Contains one transaction id when a transaction is in progress.
- Written atomically before filesystem mutation begins.

### `<txid>.json`

Metadata file example:

```json
{
  "version": 1,
  "txid": "tx-1771001234-000042",
  "operation": "upgrade",
  "status": "applying",
  "started_at_unix": 1771001234,
  "snapshot_id": "git:5f1b3d8a1f2a4d0e"
}
```

Allowed status values:

- `planning`
- `applying`
- `committed`
- `rolling_back`
- `rolled_back`
- `failed`

### `<txid>.journal`

Append-only JSON lines with deterministic step sequence:

```json
{"seq":1,"step":"backup_receipt","state":"done","path":"..."}
{"seq":2,"step":"remove_package_dir","state":"done","path":"..."}
{"seq":3,"step":"write_receipt","state":"done","path":"..."}
```

Rules:

- Every mutating step must record forward action and rollback payload before execution.
- Journal writes must be fsync-safe before moving to next step.

## Transaction Lifecycle

1. Resolve plan and preflight checks.
2. Create transaction metadata and set status `planning`.
3. Acquire process lock and write `active`.
4. Write rollback payloads for all planned mutable steps.
5. Set status `applying`.
6. Execute steps in order, journaling each completed step.
7. Set status `committed`.
8. Remove `active` and clean staging.

If any step fails during `applying`, transaction enters rollback flow.

## Rollback Semantics

Rollback executes compensating steps in reverse `seq` order.

Required rollback properties:

- Idempotent: re-running rollback after partial rollback must be safe.
- Best-effort cleanup must not hide primary errors.
- Final status must be `rolled_back` if all compensating actions succeed.
- Final status must be `failed` if any compensating action fails.

Rollback payload examples:

- Pre-mutation package directory snapshot.
- Pre-mutation receipt backup.
- Previous binary links/shims snapshot entries.
- Previous package completion files.
- Previous GUI assets and GUI ownership state.
- Previous native sidecar state for native uninstall replay.

## Crash Recovery

Before any mutating command starts, Crosspack checks for `state/transactions/active`.

If active transaction exists:

1. Load `<txid>.json` and `<txid>.journal`.
2. If status is `planning` or `applying`, auto-run rollback.
3. If status is `committed`, finalize cleanup and remove stale `active`.
4. If status is `rolling_back`, resume rollback.
5. If status is `failed`, block mutation commands and instruct user to run repair command.

Deterministic user messages:

- `recovered interrupted transaction <txid>: rolled back`
- `transaction <txid> requires repair`

## CLI Contract Additions

### `crosspack rollback`

```text
crosspack rollback [<txid>]
```

Rules:

- Without `<txid>`, rollback last non-final transaction if one exists.
- With `<txid>`, rollback that transaction if status is rollback-eligible.
- Already committed transactions are not rollback-eligible.

Output states:

- `rolled back <txid>`
- `no rollback needed`
- `rollback failed <txid>`

### `crosspack doctor`

Extend output with transaction health section:

- `transaction: clean`
- `transaction: active <txid>`
- `transaction: failed <txid>`

### `crosspack repair`

```text
crosspack repair
```

Runs deterministic recovery routine for `failed` transaction states and stale locks.

## Snapshot Consistency Binding

All install/upgrade transactions must bind to one metadata snapshot id from v0.3 source state.

Rules:

- Resolver consumes one snapshot id for the whole transaction.
- Receipts written by the transaction include `snapshot_id=<id>`.
- Mixed snapshot ids in one transaction are prohibited.

Rationale:

- Reproducibility.
- Debuggable upgrade provenance.

## Optional Artifact Signature Policy

If enabled by policy, artifact install additionally requires artifact signature verification using manifest `artifact.signature` metadata.

Policy behavior:

- Disabled by default in v0.5.
- When enabled, signature failure is fatal and triggers rollback.

## Receipt Compatibility

`InstallReceipt` adds optional field:

- `snapshot_id=<id>`

Compatibility rules:

- Missing `snapshot_id` in legacy receipts is accepted.
- New writes always include `snapshot_id` when available.

## Error Semantics

Required error classes:

- `transaction-lock-held`
- `transaction-journal-corrupt`
- `transaction-rollback-failed`
- `transaction-repair-required`
- `snapshot-id-mismatch`

Errors must include txid and step information when relevant.

## Testing Requirements

### `crosspack-installer`

- Journal write/read round-trip tests.
- Reverse-order rollback tests with simulated mid-apply failure.
- Idempotent rollback tests (run rollback twice).
- Crash recovery tests for statuses `planning`, `applying`, and `rolling_back`.

### `crosspack-cli`

- `rollback` command parsing and state-output tests.
- `doctor` transaction health output tests.
- `repair` command behavior tests.
- Rollback snapshot capture tests for completions/GUI/native sidecar payload.
- Rollback replay ordering tests for native uninstall before managed restore.

### `crosspack-resolver`

- Snapshot id propagation tests from resolve call to install plan.

### Integration

- Interrupt install after package dir write, restart command, verify automatic rollback.
- Interrupt upgrade after receipt write, restart command, verify consistent final state.
- Verify receipts after successful transaction contain one shared `snapshot_id`.

## Documentation Updates Required

- `docs/install-flow.md`: keep native/managed installer-kind contract, escalation policy semantics, and rollback snapshot coverage in sync.
- `docs/architecture.md`: keep transaction coordinator and rollback replay ordering responsibilities in sync.
- `docs/manifest-spec.md`: keep supported artifact-kind contract (`deb`/`rpm` removed) and mode defaults in sync.
