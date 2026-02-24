# CRATES KNOWLEDGE BASE

## OVERVIEW
Crates split by domain boundaries: CLI orchestration, shared models, resolver planning, installer state, registry trust, and crypto verification.

## CRATE MAP
| crate | owns | avoid |
|---|---|---|
| `crates/crosspack-cli` | command parsing, UX output contracts, command-to-crate wiring | embedding domain state logic; duplicating resolver/installer/registry rules |
| `crates/crosspack-core` | manifest/domain structs, shared types, serde-facing schemas | command behavior, IO/network side effects |
| `crates/crosspack-resolver` | dependency graph solve, ordering, constraint decisions | terminal output formatting, install transaction persistence |
| `crates/crosspack-installer` | prefix layout, transaction markers, receipts, rollback metadata | source trust decisions, dependency solve policy |
| `crates/crosspack-registry` | source records, snapshot reads, fingerprint/signature gate checks | install lifecycle mutation, CLI rendering |
| `crates/crosspack-security` | checksum helpers, signature verification primitives | source indexing policy, command routing |

## INTEGRATION HOTSPOTS
- `crates/crosspack-cli/src/main.rs` is primary seam: routes all commands into resolver/registry/installer.
- `crosspack-cli` depends on `crosspack-core` types as canonical request/response payload shapes.
- `crosspack-cli` + `crosspack-resolver`: planner outputs feed install/upgrade execution ordering.
- `crosspack-cli` + `crosspack-installer`: execution plans become persisted receipts/transaction state.
- `crosspack-cli` + `crosspack-registry`: source operations and snapshot metadata reads.
- `crosspack-registry` should call `crosspack-security` for digest/signature checks; trust gates stay centralized.
- `crosspack-installer` and `crosspack-resolver` meet at plan boundary only; keep planner pure, executor stateful.

## CHANGE COUPLING
- If manifest fields change in `crates/crosspack-core/src/lib.rs`, recheck CLI parse/display and resolver assumptions.
- If resolver ordering/constraints change, recheck installer transaction sequencing and CLI machine-readable summary lines.
- If receipt or transaction fields change in `crates/crosspack-installer/src/lib.rs`, sync CLI status output and docs that describe install flow.
- If registry source/snapshot structures change, recheck CLI `registry` commands and any installer metadata consumers.
- If signature/hash helpers change in `crates/crosspack-security/src/lib.rs`, revalidate registry verification call sites.
- New command behaviors usually require edits in `crosspack-cli` plus one domain crate; avoid spreading behavior across 3+ crates without reason.

## ANTI-PATTERNS (CRATES)
- Adding business rules in `crosspack-cli` that duplicate crate-internal policy.
- Importing `crosspack-installer` into resolver paths to "just execute while solving".
- Bypassing `crosspack-security` with ad-hoc hash/signature checks in other crates.
- Putting snapshot trust decisions in installer code.
- Coupling `crosspack-core` to concrete IO/network/runtime crates.
- Cross-crate cycles created via convenience re-exports; keep dependencies directional.
- Editing output contracts in CLI when the actual invariant belongs to resolver/installer models.
