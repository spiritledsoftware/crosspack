# Installed Identity Profile Model Spec

**Status:** roadmap, non-GA
**Related shipped docs:** `docs/architecture.md`, `docs/install-flow.md`
**Last updated:** 2026-04-29

## Problem

Crosspack has moved beyond a purely name-keyed installed package model. Current work introduces installed package identities and ambiguity checks, but lifecycle UX is still mostly optimized for one installed instance per package name.

Long term, Crosspack needs to support multiple installed identities for the same package name across target triples, profiles, source namespaces, and possibly install modes without making install, uninstall, upgrade, list, and dependency policy ambiguous or unsafe. Selector support alone is not enough: the persisted receipt, sidecar, and package payload paths must also be identity-keyed so two same-name installs cannot overwrite or delete each other.

## Goals

- Make installed package identity explicit and stable across lifecycle commands.
- Support concurrent same-name installs across target/profile/source namespace without path collisions or silent selection.
- Separate payload coexistence from exposed/active links, following Homebrew's keg/link separation rather than treating every installed identity as globally active.
- Keep legacy receipt compatibility for existing prefixes.
- Preserve deterministic plain output for automation.
- Give users short selectors for common cases and precise selectors for ambiguous cases.

## Non-Goals

- Do not introduce multi-user or system-wide shared prefix management.
- Do not change registry package identity or manifest `name` semantics.
- Do not make profiles a substitute for package variants in registry metadata.
- Do not require users to specify full identity selectors when only one match exists.

## Current State

- Implementation note: identity-keyed package storage, identity receipt fields, selector parsing, installer-owned selector resolution, identity-aware list output, identity-scoped pins, identity-scoped uninstall routing, and ambiguity diagnostics are implemented. Remaining future work should be tracked in follow-up specs or plans rather than this initial implementation plan.

- New receipts are identity-keyed; legacy name-keyed receipts remain readable in compatibility paths: `state/installed/<name>.receipt`.
- Installed-state documents can be identity-keyed and can hydrate legacy receipt/sidecar data.
- CLI lifecycle paths can detect ambiguous bare package names in some flows and return guidance.
- `InstallReceipt` still stores a single package name, version, target, mode, reason, and snapshot id.
- New package payloads are identity-keyed; legacy payloads remain readable at `pkgs/<name>/<version>/`.
- New sidecars are identity-keyed; legacy sidecars remain readable at `<name>.gui`, `<name>.services`, and `<name>.integrations`.

## Target Behavior

Lifecycle commands accept package selectors with increasing precision:

```text
crosspack uninstall ripgrep
crosspack uninstall ripgrep --target x86_64-unknown-linux-gnu
crosspack uninstall ripgrep --profile default
crosspack uninstall ripgrep --source community
crosspack uninstall ripgrep@x86_64-unknown-linux-gnu#default
```

Rules:

- A bare name succeeds only when it resolves to exactly one installed identity.
- Ambiguous selectors fail before mutation and print deterministic matching choices.
- New installs write identity-keyed receipts, package payload roots, and sidecars.
- Legacy installs remain readable and removable through compatibility paths.
- Payload storage can contain multiple same-name identities, but exposed bins/completions/GUI/services/integrations remain explicitly owned and conflict-checked.
- Upgrade groups by compatible identity dimensions rather than by package name alone.
- List output can remain compact by default, but an explicit verbose or machine mode exposes identity keys.
- Pins are scoped by identity dimension when needed, with legacy name pins applying as broad defaults.

## Architecture

The installer remains the source of truth for persisted installed identity state and the storage layout that owns each identity. The CLI parses user selectors and asks installer APIs to resolve them. The design follows three external lessons: Homebrew separates coexisting payloads from active links, pacman/paru keep a strong installed database for dependency/conflict reasoning, and winget requires source/scope/id disambiguation when metadata is not unique enough.

```text
CLI selector parser
        |
        v
installer identity resolver ----> identity-keyed receipts/state/sidecars/payloads
        |                         exposure ownership records
        |                         legacy receipts/sidecars/payloads
        v
lifecycle operation target set with storage owner
```

Responsibilities:

- `crosspack-core`: shared selector and identity structs if they become public contract types.
- `crosspack-installer`: persisted identity model, storage layout, exposure ownership layout, migration, lookup, ambiguity detection, and selected-identity cleanup.
- `crosspack-cli`: selector parsing, user guidance, output rendering.
- `crosspack-resolver`: target/profile-aware summaries for dependency and upgrade planning.

## Data/State Model

Installed identity fields:

- `name`
- `version`
- `target`
- `profile`
- `source`
- `source_namespace`
- `source_provenance`
- `install_mode`
- `snapshot_id`

Identity key requirements:

- Deterministic and filesystem-safe.
- Stable across process runs.
- Derived from persisted receipt/state fields, not from transient registry ordering.
- Compatible with legacy receipts that lack source/profile fields.
- Separate source provenance from source namespace. Provenance records where metadata came from; namespace participates in co-install identity only when explicitly selected or required to avoid a real collision.

New identity-keyed storage layout:

```text
pkgs/identities/v1/<profile>/<target>/<namespace>/<package>/<version>/
state/installed/identities/v1/<profile>/<target>/<namespace>/<package>.receipt
state/installed/identities/v1/<profile>/<target>/<namespace>/<package>.state.json
state/installed/identities/v1/<profile>/<target>/<namespace>/<package>.gui
state/installed/identities/v1/<profile>/<target>/<namespace>/<package>.gui-native
state/installed/identities/v1/<profile>/<target>/<namespace>/<package>.services
state/installed/identities/v1/<profile>/<target>/<namespace>/<package>.integrations
state/pins/identities/v1/<profile>/<target>/<namespace>/<package>.pin
```

Path components must be escaped with a single canonical filesystem-safe encoding. If path length becomes a practical problem, Crosspack can replace the readable path tail with a short hash while keeping the full readable identity inside `.state.json`.

Legacy compatibility layout:

```text
pkgs/<name>/<version>/
state/installed/<name>.receipt
state/installed/<name>.state.json
state/installed/<name>.gui
state/installed/<name>.gui-native
state/installed/<name>.services
state/installed/<name>.integrations
state/pins/<name>.pin
```

Receipts written in the identity-keyed layout include identity fields:

```text
identity_profile=default
identity_target=x86_64-unknown-linux-gnu
identity_source=community
identity_source_namespace=community
identity_source_provenance=community
identity_package=demo
```

Compatibility rules:

- Missing `identity_*` fields hydrate from old receipt fields.
- Old `target=` remains accepted.
- New writes include `identity_*` fields.
- Identity-keyed state documents are preferred over legacy documents when both exist for the same identity.

Exposure ownership records are distinct from storage ownership:

```text
state/exposures/bin/rg.owner
state/exposures/completions/bash/rg.owner
state/exposures/gui/<key>.owner
state/exposures/services/<name>.owner
state/exposures/integrations/<kind>/<key>.owner
```

Each exposure owner file stores the installed identity that owns the visible projection. This allows multiple payloads to coexist while preventing two identities from silently fighting over `bin/rg` or a service name. A later active-pointer layer can point from profile/package to the selected identity:

```text
state/active/<profile>/<package>.identity
```

Active pointers are useful for default UX but are not a substitute for exposure ownership checks.

Migration rules:

- Legacy receipts hydrate as `profile=default`, `source_namespace=default`, and `source_provenance=unknown` unless source can be inferred safely.
- Existing name-keyed receipts, sidecars, and payload paths remain readable and removable for at least one compatibility window.
- New writes produce identity-keyed receipts, state documents, sidecars, and package payload directories.
- Normal reads do not automatically move legacy installs; a future `repair` or `migrate-state` flow can convert legacy installs explicitly.

## CLI/UX Contracts

Ambiguity error shape should be deterministic and actionable:

```text
package name 'ripgrep' is ambiguous; specify one of:
  ripgrep --target x86_64-unknown-linux-gnu --profile default
  ripgrep --target aarch64-apple-darwin --profile default
```

Plain list extensions should be opt-in:

```text
crosspack list --identity
ripgrep 14.1.1 target=x86_64-unknown-linux-gnu profile=default source=community
```

Existing `crosspack list` output must remain stable unless a coordinated contract migration is planned.

## Failure Modes

- Ambiguous bare name: fail before mutation.
- Legacy receipt with insufficient identity data: hydrate into default compatibility identity and warn only in diagnostic commands.
- Identity document conflicts with receipt data: fail closed for mutation commands; `doctor` reports repair guidance.
- Multiple state documents claim the same key: fail closed and require repair.
- Identity-keyed receipt and package directory disagree: fail closed for mutation commands; `doctor` reports repair guidance.
- Identity-keyed sidecar cleanup targets a legacy path or different identity: fail before deleting user-visible state.
- Exposure owner disagrees with target identity: fail preflight before overwriting or deleting the visible projection.

## Testing Requirements

- Hydrate legacy receipts into default identity values.
- Resolve exact selector to one installed identity.
- Reject ambiguous bare-name uninstall, upgrade, pin, and service commands.
- Verify list output remains stable by default.
- Verify identity-aware list output is sorted deterministically.
- Verify uninstall removes identity-keyed and legacy state records.
- Verify two same-name identities with the same version install into separate package roots.
- Verify uninstall with a precise selector removes only the selected identity's receipt, payload, and sidecars.
- Verify reinstall of one identity does not remove another same-name identity's GUI/service/integration state.
- Verify two same-name identities can coexist while only one owns a conflicting exposed binary.
- Verify source provenance changes do not create a second identity unless source namespace is explicitly selected or required.
- Verify rollback snapshot capture/restore uses the selected identity storage owner.
- Verify upgrade planning does not merge same-name identities across incompatible targets.

## Rollout Plan

1. Add identity fields, selectors, and compatibility hydration.
2. Add identity-keyed receipt, package payload, sidecar, pin, state-document, exposure-owner, and optional active-pointer paths.
3. Make new installs write identity-keyed storage while reads preserve legacy compatibility.
4. Add identity-aware read APIs and ambiguity tests for all lifecycle commands.
5. Add opt-in list/status output exposing identities.
6. Add identity-scoped pin, uninstall, rollback, and upgrade semantics.
7. Add repair diagnostics for conflicting identity or storage-owner state.

## Open Questions

- Should profile be user-facing in v1, or reserved for future prefix/profile work?
- Should selectors use flags only, compact syntax only, or both?
- Should active pointers be implemented in the first identity-storage PR, or after exposure-owner behavior lands?
- Should a migration command move legacy payloads eagerly, or should legacy state remain lazy until reinstall/uninstall?
