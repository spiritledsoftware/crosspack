# RESOLVER KNOWLEDGE BASE

## OVERVIEW
Backtracking dependency resolver centered on `resolve_dependency_graph`, with complexity concentrated in recursive `search` + deterministic `topo_order`.

## WHERE TO LOOK
- Entry API: `src/lib.rs` -> `resolve_dependency_graph`.
- Version pick helper: `src/lib.rs` -> `select_highest_compatible`.
- Backtracking core: `src/lib.rs` -> `search`.
- Candidate filtering + load cache: `src/lib.rs` -> `matching_candidates`.
- Constraint/pin consistency check: `src/lib.rs` -> `selected_satisfies_constraints`.
- Install ordering + cycle errors: `src/lib.rs` -> `topo_order`.
- Behavioral spec: `src/lib.rs` test module (`applies_pin_to_transitive_dependency_constraints`, `fails_on_pin_conflict`, `fails_on_cycle`).

## CONVENTIONS
- Constraint stacking stays in `constraints: BTreeMap<String, Vec<VersionReq>>`; `search` must push dependency reqs, recurse, then rollback via `truncate` + `retain`.
- Candidate filtering stays centralized in `matching_candidates`; keep both filters explicit: all stacked `package_reqs` and optional `pin_req`.
- Cache behavior stays local to `matching_candidates` through `versions_cache`; load once per package name before filtering.
- Consistency gate stays cheap and side-effect free in `selected_satisfies_constraints`; call before deeper recursion.
- `search` package selection order is map-key deterministic (`constraints.keys().find(...)`); preserve determinism for tests and CLI output stability.
- Error reporting uses concrete `anyhow!` strings from resolver internals; keep message shapes stable for assertions and UX.
- `topo_order` must keep deterministic cycle node listing (`cycle_nodes.sort()`), then emit one joined error line.

## ANTI-PATTERNS
- Splitting constraint-stack logic across helpers; breaks rollback invariants in `search`.
- Adding early-return branches in `search` that skip `selected.remove(&next)` or constraint rollback.
- Moving candidate filtering out of `matching_candidates` or filtering pins/requirements in separate passes elsewhere.
- Returning generic failures where `matching_candidates` currently reports exact conflicts (`constraints [...]`, optional `pin ...`).
- Rewording resolver errors without need; tests and callers rely on fragments like `package '{name}' was not found`, `no matching version`, `dependency cycle detected`.
- Introducing non-deterministic containers/order in resolution or topo traversal paths.
