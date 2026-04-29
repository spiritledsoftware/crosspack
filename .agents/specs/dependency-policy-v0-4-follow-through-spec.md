# Dependency Policy v0.4 Follow-Through Spec

**Status:** roadmap, non-GA
**Related shipped docs:** `docs/dependency-policy-spec.md`, `docs/install-flow.md`
**Last updated:** 2026-04-29

## Problem

Crosspack has core support for typed dependency policy evidence: provider substitutions, conflicts, replacements, deterministic plan rendering, and replacement handoff. The remaining work is to make the policy complete, stable under upgrade, exercised by real registry packages, and easy to reason about when failures happen.

## Goals

- Complete the v0.4 policy behavior described by `docs/dependency-policy-spec.md`.
- Keep provider selection deterministic and stable across upgrades.
- Ensure conflicts and replacements are represented in typed plans, not ad hoc CLI strings.
- Build registry package coverage that catches real policy regressions.
- Preserve existing machine-oriented output contracts.

## Non-Goals

- Do not replace the current resolver with a SAT solver.
- Do not add interactive conflict resolution.
- Do not auto-replace unrelated root packages.
- Do not introduce arbitrary lifecycle scripting for replacement migration.

## Current State

- Manifest schema supports `provides`, `conflicts`, and `replaces`.
- Resolver can select capability providers and expose plan evidence.
- CLI can render provider/conflict/replacement explainability lines.
- Replacement handoff uses typed plan data and preserves root intent.
- Some policy behavior remains stronger in tests than in real registry coverage.

## Target Behavior

Provider selection:

- Direct package name wins over capability provider candidates.
- Provider candidates sort deterministically by version, source precedence, and package name.
- During upgrade, the currently installed provider should be preferred when it still satisfies constraints, pins, and conflict policy.

Conflict policy:

- Conflicts are checked both within the selected graph and against installed packages not being removed.
- Errors include selected package, conflicting package, versions, and requirement.
- Plans expose conflicts as typed evidence for renderers and diagnostics.

Replacement policy:

- Replacements are explicit manifest contracts only.
- Replacement targets must be removed before the new package claims conflicting assets.
- Replacement handoff must fail before mutation if remaining roots still require the replaced package by name.
- Missing replacement receipts during grouped apply are tolerated when an earlier step already removed the target.

## Architecture

Policy evidence flows from resolver to CLI and installer through typed plan structs.

```text
registry manifests + installed summaries
        |
        v
crosspack-resolver
        |
        v
InstallPlan { packages, removals, replacements, conflicts, providers }
        |
        +--> CLI preview/explain output
        |
        +--> installer apply preflight and handoff
```

The resolver should not import installer state. The CLI converts installer receipts/state into resolver summaries.

## Data/State Model

Typed plan evidence should remain the durable planning boundary:

- `PlannedPackage`: selected package, version, target, install reason, dependencies.
- `PlannedRemoval`: package removed and reason.
- `PlannedReplacement`: removed package, replacement package, requirement.
- `ProviderSubstitution`: requested capability, selected provider, provider version.
- `ConflictConstraint`: selected package, conflicting package, requirement.

Future additions should be additive fields, not string parsing of rendered output.

## CLI/UX Contracts

Dry-run output must preserve existing contract tokens:

- `transaction_preview`
- `transaction_summary`
- `risk_flags`
- `change_add`
- `change_remove`
- `change_replace`
- `change_transition`

Explainability output may add stable lines, for example:

```text
explain_provider capability=c-compiler selected=clang version=18.0.0
explain_conflict selected=foo conflicts_with=bar requirement=<2.0.0
explain_replacement selected=clang@18.0.0 removes=old-cc@1.5.0 declared=<2.0.0
```

Provider override errors must identify unused, invalid, and non-provider overrides separately.

## Failure Modes

- Provider not found: fail resolution with capability name and requirement.
- Provider override invalid: fail before resolution proceeds.
- Conflict with installed package: fail before mutation.
- Replacement blocked by remaining roots: fail before mutation.
- Replacement asset ownership collision: fail before mutation.

## Testing Requirements

- Resolver prefers currently installed provider during upgrade when valid.
- Resolver switches provider only when required by constraints, pins, or conflicts.
- Replacement removals are deduplicated across grouped plans.
- Missing replacement receipt during sequential apply does not abort.
- Conflict evidence is deterministic and sorted.
- CLI dry-run contract output is unchanged for existing scenarios.
- Registry fixture packages exercise provider, conflict, and replacement paths.

## Rollout Plan

1. Add provider stability tests and resolver behavior.
2. Expand registry fixtures with small policy packages.
3. Add end-to-end dry-run and apply tests for policy packages.
4. Audit CLI output for stable explainability lines.
5. Update shipped docs when the planned behavior is complete and tested.

## Open Questions

- Should provider stability be absolute when valid, or should a newer provider win by default unless pinned?
- Should replacement of root packages require an explicit CLI flag in some cases?
- Should policy package fixtures live in the main registry or a dedicated test registry fixture?
