# Dependency Policy v0.4 Follow-Through Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Do not implement directly from `.agents/specs/dependency-policy-v0-4-follow-through-spec.md`; this plan is the implementation boundary.

**Goal:** Complete the next small, reviewable slice of dependency-policy behavior while preserving existing machine-oriented output contracts and keeping resolver policy evidence typed.

**Architecture:** Keep dependency solving in `crosspack-resolver`. Keep manifest schema in `crosspack-core`. Keep installation state and filesystem mutation in `crosspack-installer`. Keep CLI rendering and command orchestration in `crosspack-cli`. Policy evidence should cross crate boundaries through typed plan structs, not parsed strings.

**Tech Stack:** Rust workspace, existing resolver backtracking implementation, `PackageManifest`, `InstallPlan`, Cargo tests/clippy.

---

## Current Context

Already implemented on `main`:

- `PackageManifest` parses `provides`, `conflicts`, and `replaces` in `crates/crosspack-core/src/manifest.rs`.
- `crosspack-resolver` resolves capability providers when no direct package exists.
- Direct package names win over provider candidates.
- Provider candidates are deterministic by version descending, then package name ascending.
- Conflict checks reject conflicts within the selected graph and against installed packages passed by the caller.
- `InstallPlan` carries typed `ProviderSubstitution`, `ConflictConstraint`, and `PlannedReplacement` evidence.
- CLI tests cover deterministic explainability output for providers, conflicts, and replacements.
- Identity-profile work has landed on `main`; local `.brv` context files remain uncommitted and should not be touched by this plan.

Important current limitation:

- Provider selection does not yet intentionally prefer an already-installed provider during upgrade when that provider still satisfies all constraints, pins, and conflict policy.

## Scope For This PR

Implement only provider stability during upgrade.

This means:

- For capability dependencies, prefer the installed provider package/version when it is still a valid candidate.
- Do not change direct package dependency precedence.
- Do not change conflict/replacement semantics in this PR.
- Do not introduce source precedence metadata unless an existing source-order field is already available.
- Do not change CLI output line shapes.
- Do not add registry fixture packages in this PR unless resolver/CLI tests reveal a necessary fixture gap.

## Non-Goals

- No SAT solver rewrite.
- No interactive conflict resolution.
- No provider override behavior changes.
- No replacement root-package policy changes.
- No install transaction or receipt schema changes.
- No docs claiming the full v0.4 policy is complete.

---

## Implementation Plan

### Task 1: Lock Current Provider Behavior With Tests

**Files:**
- Modify: `crates/crosspack-resolver/src/tests.rs`

- [x] Add a resolver test showing an installed provider is preferred over a newer provider for a capability dependency when the installed provider still satisfies the dependency requirement.
- [x] Add a resolver test showing the resolver switches away from the installed provider when the installed provider no longer satisfies the capability dependency requirement.
- [x] Add a resolver test showing direct package names still beat provider candidates even when another installed package provides the same name as a capability.
- [x] Add a resolver test showing an installed provider is not preferred when it violates a pin.

Expected test names:

- `prefers_installed_provider_for_capability_when_valid`
- `switches_provider_when_installed_provider_no_longer_satisfies_constraints`
- `direct_package_dependency_still_wins_over_installed_provider`
- `does_not_prefer_installed_provider_when_pin_excludes_it`

### Task 2: Implement Provider Stability In Resolver Candidate Ordering

**Files:**
- Modify: `crates/crosspack-resolver/src/search.rs`

- [x] Thread the existing `installed: &BTreeMap<String, PackageManifest>` argument into provider candidate ordering only.
- [x] For capability candidate ordering, rank a candidate first when `installed` contains the same package name, the installed version equals the candidate version, and the installed manifest provides the requested capability.
- [x] Preserve existing ordering for all other candidates: highest version first, then lexicographically smallest package name.
- [x] Do not apply this ranking when a direct package match exists for the dependency token.
- [x] Keep the helper private to `search.rs` unless another resolver module needs it.

### Task 3: Preserve Typed Plan Evidence

**Files:**
- Modify only if tests expose a gap: `crates/crosspack-resolver/src/plan.rs`
- Modify only if needed: `crates/crosspack-resolver/src/tests.rs`

- [x] Verify `provider_substitutions(graph, root_names)` still reports the selected provider after stability ordering.
- [x] Add a plan-level test only if current resolver tests do not exercise the typed `ProviderSubstitution` evidence for an installed-stable provider.
- [x] Do not add string-only evidence.

### Task 4: Update Shipped Documentation Conservatively

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/install-flow.md`
- Modify only if needed: `docs/dependency-policy-spec.md`

- [x] Document that provider selection keeps an already-installed valid provider during upgrade to avoid unnecessary churn.
- [x] Keep wording scoped to shipped behavior.
- [x] Do not mark the entire v0.4 dependency-policy roadmap complete.

### Task 5: Validate

Run focused checks first:

```sh
cargo fmt --all --check
cargo test -p crosspack-resolver
cargo clippy -p crosspack-resolver --all-targets -- -D warnings
```

Run broader checks if resolver changes touch public plan shapes or docs/CLI behavior:

```sh
cargo test -p crosspack-cli
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Always run before handoff:

```sh
git diff --check
```

---

## Review Checklist

- [x] Only provider stability is implemented.
- [x] Direct package precedence is unchanged.
- [x] Provider candidate ordering remains deterministic.
- [x] Existing CLI output contract tokens are unchanged: `transaction_preview`, `transaction_summary`, `risk_flags`, `change_add`, `change_remove`, `change_replace`, `change_transition`.
- [x] `.brv` context files remain untouched by implementation work.
- [x] Registry submodule is not manually bumped.

## Follow-Up Roadmap Items

- [x] Add registry fixture packages for provider/conflict/replacement paths.
- [x] Add end-to-end dry-run/apply tests over policy packages.
- [x] Audit provider override errors for distinct unused, invalid, and non-provider cases.
- [x] Revisit replacement root-package policy after provider stability ships.

Completion notes:

- Registry-backed provider fixture coverage uses signed configured-source manifests and exercises capability lookup through `MetadataBackend::dependency_versions`.
- CLI dry-run explainability coverage now reaches provider fixtures through normal configured metadata resolution, not resolver-only synthetic maps.
- Provider override errors distinguish invalid shape/token, unused overrides, unknown provider packages, non-provider packages, and direct-package override attempts.
- Replacement root policy was already implemented and covered by handoff tests for blocked dependents, interdependent replacement roots, planned dependency overrides, and missing replacement receipts.
