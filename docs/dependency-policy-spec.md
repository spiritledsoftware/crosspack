# Dependency Policy and Provider Resolution Specification (Draft v0.4)

This document defines v0.4 dependency-policy behavior for Crosspack. It introduces package conflict policy, package replacement policy, and virtual capability providers while preserving deterministic resolution.

**Status:** roadmap draft (non-GA). This document is a design target and does not change shipped GA guarantees until implementation is merged and released.

## Scope

This spec covers:

- Manifest schema additions for `provides`, `conflicts`, and `replaces`.
- Resolver behavior for capability providers.
- Conflict and replacement policy across install and upgrade.
- CLI and receipt impacts.

This spec does not cover:

- Service lifecycle management.
- Arbitrary pre/post install scripting.
- Distro-level package epoch semantics.

## Goals

- Keep package installation flow predictable and deterministic.
- Allow virtual capabilities similar to mature package ecosystems.
- Prevent unsafe package combinations via explicit conflict rules.
- Support controlled package handoff through replacement rules.

## Non-Goals

- No SAT solver rewrite in v0.4.
- No interactive conflict resolution.
- No automatic replacement of unrelated root packages.

## Manifest Schema

`PackageManifest` adds these optional fields:

```toml
name = "toolchain"
version = "2.1.0"

provides = ["c-compiler", "posix-cc"]

[dependencies]
c-compiler = ">=2.0.0, <3.0.0"

[conflicts]
legacy-cc = "*"

[replaces]
old-toolchain = "<2.0.0"
```

### Field Definitions

- `provides`: array of capability names.
  - Capabilities use package-name grammar: `^[a-z0-9][a-z0-9._+-]{0,63}$`.
  - Capability version is the provider package version.
- `conflicts`: map of package-name to semver requirement.
  - Means this package cannot coexist with matching installed or selected versions.
- `replaces`: map of package-name to semver requirement.
  - Means this package can replace matching installed package state during install/upgrade.

Validation rules:

- `conflicts` key equal to manifest `name` is invalid.
- `replaces` key equal to manifest `name` is invalid.
- Invalid semver requirement in `conflicts` or `replaces` is a parse error.

## Resolver Semantics

`resolve_dependency_graph` keeps existing backtracking flow and extends candidate matching.

### Dependency Lookup

Given dependency token `dep_name`:

1. If package manifests exist with `name == dep_name`, resolve as direct package dependency.
2. Otherwise, resolve as capability dependency against manifests where `provides` contains `dep_name`.
3. Requirement matching always compares against provider package version.

This preserves explicit package naming while enabling virtual capabilities.

### Candidate Ordering

Candidate ordering must be deterministic:

1. Highest package version first.
2. Source precedence from v0.3 if versions tie.
3. Lexicographically smallest package name as final tie-break.

### Provider Stability

When upgrading, resolver should prefer currently installed provider if it still satisfies all constraints and pins.

Rationale:

- Avoid provider churn across upgrades.
- Keep behavior predictable in long-lived installations.

## Conflict Policy

A selected package `A` conflicts with `B` when:

- `A.conflicts` has key `B.name` and requirement matches `B.version`, or
- `B.conflicts` has key `A.name` and requirement matches `A.version`.

Conflict checks apply to:

- Packages in the newly resolved graph.
- Already installed packages not planned for replacement/removal.

Install/upgrade fails on unresolved conflicts with actionable message:

- `cannot install <A>: conflicts with installed <B> <version>`
- `dependency resolution failed: conflict between <A> and <B>`

## Replacement Policy

Replacement is an explicit compatibility contract.

Package `A` may replace installed package `B` when:

- `A.replaces[B.name]` exists and matches installed `B.version`.
- No third package requires `B` by name in final resolved graph.
- Binary ownership transfer preflight succeeds.

Behavior:

1. Solver marks `B` as replacement target.
2. Installer removes `B` package directory, receipt, and owned binaries.
3. Installer proceeds with `A` installation.
4. If replaced package had `install_reason=root`, replacement package is written as `install_reason=root`.

If any replacement precondition fails, operation fails before filesystem mutation.

## CLI Contract Changes

### Provider Override

`install` and single-package `upgrade` accept manual provider binding:

```text
crosspack install <name[@constraint]> --provider <capability>=<package>
```

Rules:

- Repeatable `--provider` allowed.
- Override must reference a capability requested directly or transitively.
- Override package must actually provide that capability.
- Invalid override causes hard error.

### Informational Output

When a capability dependency is resolved, CLI emits deterministic note:

- `selected provider <package> for capability <capability>`

`info <name>` displays optional sections when present:

- `Provides:`
- `Conflicts:`
- `Replaces:`

## Receipt and State Impacts

`InstallReceipt` adds optional repeated fields:

- `provided_capability=<capability>`
- `replaced_package=<name@version>`

Compatibility rules:

- Missing fields parse as empty vectors.
- Existing receipts from v0.2/v0.3 remain valid.

## Error Semantics

Required error classes:

- `provider-not-found`
- `provider-override-invalid`
- `dependency-conflict`
- `replacement-preflight-failed`
- `replacement-blocked-by-dependents`

All errors must include package names and constraints where applicable.

## Testing Requirements

### `crosspack-core`

- Parse tests for `provides`, `conflicts`, `replaces`.
- Validation tests for self-conflict and self-replace rejection.

### `crosspack-resolver`

- Capability dependency resolves to provider when no direct package exists.
- Direct package name takes precedence over capability.
- Deterministic tie-break across providers.
- Conflict detection within resolved graph.
- Conflict detection against installed state constraints provided by caller.

### `crosspack-cli`

- `--provider` parsing and validation tests.
- Stable output tests for selected provider messages.
- `info` rendering tests for new manifest fields.

### `crosspack-installer`

- Replacement preflight tests for binary ownership collisions.
- Root-intent preservation tests when replacement occurs.
- Dependency-blocked replacement rejection tests.

### Integration

- End-to-end capability install with deterministic provider selection.
- End-to-end replacement path preserving root install reason.
- End-to-end conflict failure with no side effects.

## Documentation Updates Required

- `docs/manifest-spec.md`: add field-level schema and examples.
- `docs/install-flow.md`: add replacement preflight and application steps.
- `docs/architecture.md`: document resolver conflict/provider phases.
