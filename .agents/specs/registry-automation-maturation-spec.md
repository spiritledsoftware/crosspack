# Registry Automation Maturation Spec

**Status:** roadmap, non-GA
**Last updated:** 2026-04-29

## Problem

Crosspack's registry is becoming an automation pipeline rather than a hand-maintained package list. Several upstream source strategies and quality gates exist, but long-term reliability requires more consistent state caching, rate-limit handling, proportional validation, auditable generated changes, and clear separation between registry-only changes and root product releases.

## Goals

- Automate routine upstream package updates safely.
- Keep generated registry changes reviewable and reproducible.
- Make quality gates proportional to changed packages.
- Handle upstream API errors and rate limits without noisy failures.
- Keep registry-only changes from triggering root product releases unless paired with product code.

## Non-Goals

- Do not auto-merge unvalidated package updates.
- Do not require every package to share one source strategy.
- Do not couple registry release cadence to Crosspack binary release cadence.
- Do not accept unsigned or unverifiable metadata as a shortcut for automation.

## Current State

- Registry supports signed package metadata and snapshot verification.
- Automation exists for multiple upstream source kinds.
- Quality gates and smoke installs have been improved for changed manifests.
- Root repo workflow filtering treats registry-only changes separately from product-impacting changes.
- Some release-bot state, rate-limit, and retry behavior has been addressed incrementally.

## Target Behavior

Registry automation should behave like an audited pipeline:

1. Discover upstream versions.
2. Compare against cached state.
3. Generate package/release metadata deterministically.
4. Sign updated sidecars.
5. Run proportional validation for changed packages.
6. Open or update a PR with a concise generated summary.
7. Auto-merge only after required checks pass and policy allows.

## Architecture

```text
upstream APIs/indexes
        |
        v
source strategy adapters
        |
        v
registry generation state cache
        |
        v
metadata writer + signer
        |
        v
quality gate + smoke install
        |
        v
PR automation
```

Source strategy adapters should be typed and testable. Generated files should be deterministic for the same upstream inputs.

## Data/State Model

Automation state should track:

- upstream source kind,
- package name,
- last checked version/revision,
- last successful generated artifact metadata,
- ETag or cache validators when available,
- last failure reason code,
- rate-limit reset time when provided.

State must not contain signing secrets or credentials.

## CLI/UX Contracts

Registry automation commands should emit deterministic summaries:

```text
registry_update package=ripgrep status=updated from=14.1.0 to=14.1.1 source=github-release
registry_update package=fd status=up-to-date version=10.2.0 source=github-release
registry_update package=zig status=failed reason=rate-limited reset_at=2026-04-29T18:00:00Z
```

Root repo release workflows should continue to treat registry submodule bumps as non-release changes unless product code changes are included.

## Failure Modes

- Upstream API 403/rate-limited: record deterministic reason and retry later.
- Upstream asset missing: fail that package without blocking unrelated package checks.
- Signature generation failure: fail the PR and do not update snapshots.
- Smoke install transient download failure: bounded retry with reason-coded final failure.
- Generated diff includes unrelated packages: fail proportional quality gate.

## Testing Requirements

- Source strategy tests with fixture upstream responses.
- State cache read/write tests.
- Rate-limit and transient failure tests.
- Proportional quality gate tests for changed manifests only.
- Deterministic generation tests.
- Signature sidecar validation tests.
- End-to-end automation dry-run tests that do not require secrets.

## Rollout Plan

1. Inventory existing source strategies and normalize their state/error outputs.
2. Add a shared automation state schema.
3. Convert package update scripts to typed strategy adapters.
4. Expand proportional validation and smoke-install retry behavior.
5. Add PR summary generation and automerge policy checks.

## Open Questions

- Should automation state live in the registry repo, root repo, or external CI cache?
- Which package classes are safe for automerge after green checks?
- How should signing be handled for scheduled automation without exposing secrets broadly?
