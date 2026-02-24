# CROSSPACK-INSTALLER KNOWLEDGE BASE

## OVERVIEW
Installer crate owns prefix filesystem layout and state transitions for install/uninstall transactions, including receipt durability and rollback recovery signals.

## CORE TYPES
- `PrefixLayout`: canonical path builder for `pkgs/`, `bin/`, `state/`, `cache/`, `share/completions/`, and transaction/pin/receipt files.
- `InstallReceipt`: persisted package state (`name`, `version`, deps, target, artifact origin/hash, cache path, exposed links, snapshot, reason, status, timestamp).
- `InstallReason`: root vs dependency classification; used by uninstall graph logic to protect reachable roots.
- `TransactionMetadata`: per-tx JSON record (`txid`, operation, status, start time, optional snapshot).
- `TransactionJournalEntry`: append-only line records for step/state/path progress.
- `UninstallResult` + `UninstallStatus`: reports blocked roots, stale-state repair, and dependency pruning outcomes.

## STATE + LAYOUT
- Base dirs created by `PrefixLayout::ensure_base_dirs()`; callers must run this before any read/write flow.
- Package payloads live at `pkgs/<name>/<version>`; exposed executables are linked/shimmed into `bin/`.
- Receipts are one file per package at `state/installed/<name>.receipt`; parsing tolerates old receipt shapes where possible.
- Pins live at `state/pins/<name>.pin`; empty pins are treated as absent.
- Transaction state lives under `state/transactions/`:
- `active`: single-writer lock marker containing active `txid`.
- `<txid>.json`: metadata document; status updates rewrite this file.
- `<txid>.journal`: newline-delimited serialized journal entries.
- `staging/<txid>/`: tx-scoped staging area coupled to metadata creation.
- Installer temp extraction uses `state/tmp/<prefix>-<pid>-<ts>` and is cleaned best-effort.
- Artifact cache is rooted at `cache/artifacts/<name>/<version>/<target>/artifact.<ext>` and pruned only through validated safe paths.
- Completion files are copied into `share/completions/packages/<shell>/...`; empty completion dirs are pruned upward.

## CHANGE IMPACT
- Path schema changes in `PrefixLayout` are breaking for receipts, uninstall cleanup, cache pruning, and CLI state inspection.
- Receipt field additions/removals must preserve parse compatibility and sort stability in `read_install_receipts()`.
- Transaction file format changes affect recovery and operator debugging; update both serializer and parser together.
- Uninstall dependency traversal changes alter `BlockedByDependents` behavior and automatic dependency pruning decisions.
- Binary/completion exposure changes impact rollback correctness because uninstall relies on receipt-recorded exposed artifacts.
- Cache pruning guardrails (`safe_cache_prune_path`) are security-sensitive; broadening acceptance can permit unintended deletions.

## ANTI-PATTERNS (INSTALLER)
- Do not write outside `PrefixLayout`-derived paths or bypass path validation helpers.
- Do not delete `state/transactions/active` without corresponding transaction completion/abort handling.
- Do not mutate receipt semantics without updating both write and parse paths in this crate.
- Do not treat dependency overrides as persisted state; they are planning-time inputs only.
- Do not remove package dirs before confirming uninstall is not blocked by remaining roots.
- Do not prune cached artifacts by raw receipt string paths unless `safe_cache_prune_path()` accepts them.

## VERIFICATION
- `rustup run stable cargo test -p crosspack-installer`
- `rustup run stable cargo clippy -p crosspack-installer --all-targets -- -D warnings`
- `rustup run stable cargo build -p crosspack-installer --locked`
- Validate tx lifecycle: claim active marker, write metadata/journal, update status, then clear active marker.
- Validate uninstall edge cases: blocked roots, stale package dir repair, and dependency prune set determinism.
- Validate exposure cleanup: removed bins/completions and completion directory pruning after uninstall.
