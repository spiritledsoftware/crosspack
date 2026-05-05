# Registry Automation Maturation Spec

**Status:** roadmap, non-GA
**Last updated:** 2026-05-04

## Problem

Crosspack's core registry is becoming an autonomous package-ingestion pipeline rather than a hand-maintained package list. The current registry automation can discover upstream versions, generate manifests, run quality gates, and open package PRs, but it still has maturity gaps:

- automation state needs to be durable and auditable,
- one malformed package can block unrelated registry/client operations,
- rate-limit and transient upstream failures should not stall the whole run,
- generated PRs should converge as a rolling automation branch instead of fragmenting by package,
- root repo release behavior must remain decoupled from registry-only movement.

This spec intentionally treats the core registry as fully autonomous and inherently unsafe. A future safer/secure registry can be monetized by applying stricter trust, review, and curation rules. The core registry should optimize for autonomous package freshness, deterministic generated changes, and blast-radius isolation.

## Goals

- Automate routine upstream package updates with broad automerge after required checks pass.
- Keep generated registry changes reviewable and reproducible.
- Commit durable automation state in the registry repo so bot behavior is auditable across runners.
- Prevent one malformed package or release record from poisoning the whole registry or Crosspack client update flow.
- Make quality gates proportional to changed packages and quarantine failures at package scope.
- Handle upstream API errors and rate limits with per-package/source backoff instead of noisy whole-run failures.
- Accumulate scheduled automation output in one rolling bot PR regenerated from current `main`.
- Preserve merge-time signing with the existing signing workflow.
- Allow source snapshots to become ready when source-level trust passes but some signed package records are malformed.
- Keep registry-only changes from triggering root product releases unless paired with product code.

## Non-Goals

- Do not make the core registry conservative or human-curated by default.
- Do not block all automation because one package is malformed, rate-limited, or transiently unavailable.
- Do not require every package to share one source strategy.
- Do not couple registry release cadence to Crosspack binary release cadence.
- Do not store signing secrets, GitHub tokens, or upstream credentials in committed automation state.
- Do not move signing into scheduled release discovery while the merge-time signing workflow exists.
- Do not weaken source-level trust checks: configured sources still require pinned `registry.pub`, ready snapshots, and valid signatures for every metadata file that exists in the snapshot.

## Current State

- Registry metadata supports signed package templates, signed release manifests, and snapshot verification.
- The registry submodule contains source-driven automation in `scripts/upstream-release-bot.py`.
- The registry repo already has committed state at `state/upstream-release-bot.json`.
- `upstream-release-bot.yml` runs hourly and can open PRs.
- `registry-quality-gate.yml` and registry validation scripts check generated manifests.
- `sign-manifests-on-merge.yml` signs changed manifests after merge.
- Registry `main` pushes already trigger a root repo submodule bump.
- Root release workflow filtering already treats registry-only submodule bumps as non-release changes.

## Target Behavior

Registry automation behaves like an audited, autonomous pipeline:

1. Start each scheduled run from current registry `main`.
2. Load committed package/source automation state from `state/upstream-release-bot.json`.
3. Discover upstream versions with conditional requests and per-package/source backoff.
4. Generate deterministic package and release metadata for candidate updates.
5. Validate each candidate package/release pair before adding it to the rolling PR.
6. Add or update quarantine records for malformed package-level failures.
7. Clear quarantine records when a later run regenerates valid metadata and package validation passes.
8. Open or update one rolling bot PR that accumulates all valid generated changes and state changes.
9. Enable broad automerge on that PR after required checks pass.
10. Let the merge-time signing workflow update `.toml.sig` sidecars after merge.
11. Let the existing registry-to-root submodule bump path update the root repo without triggering a product release.

## Architecture

```text
upstream APIs/indexes
        |
        v
typed source strategy adapters
        |
        v
committed automation state + quarantine records
        |
        v
deterministic metadata writer
        |
        v
package-scoped validation + smoke/install probes
        |
        v
rolling bot PR regenerated from registry main
        |
        v
merge-time signing workflow
        |
        v
registry main push -> existing root submodule bump
```

Source strategy adapters should be typed and testable. Generated files should be deterministic for the same upstream inputs. The bot must write only approved generated paths: `packages/*.toml`, `releases/*/*.toml`, and `state/upstream-release-bot.json`.

The Crosspack root repo also needs reader hardening in `crosspack-registry`: package-level poison should be skipped or isolated when listing/searching/loading unrelated packages. Source-level trust failures remain fatal.

`source_sync.rs` must split source readiness validation into two layers. Source-level trust validation remains mandatory and fatal for the whole source. Signed package-level semantic poison is recorded as diagnostics and does not prevent `snapshot.json` from becoming ready.

## Trust And Poison Taxonomy

Fatal source-level trust failures always fail closed for the source:

- missing `registry.pub`,
- configured fingerprint mismatch,
- missing or invalid package template signature sidecar,
- missing or invalid release manifest signature sidecar,
- missing or invalid community recipe catalog signature,
- missing or invalid ready `snapshot.json`,
- unreadable source layout or cache replacement failure.

Skippable package-level poison is limited to signed metadata whose bytes are trusted but whose package content cannot be used safely:

- signed package or release TOML syntax error,
- signed merged manifest schema/serde error,
- signed metadata missing required package fields,
- signed metadata with unsupported artifact structure,
- signed metadata for a package already listed in durable quarantine state.

Broad operations such as source sync, search, list, outdated, dependency/provider discovery, and unrelated install resolution skip skippable package poison with reason-coded diagnostics. Direct selected package operations remain strict: installing or inspecting the poisoned package fails with a package-specific error.

## Data/State Model

Durable automation state lives in the registry repo under `state/upstream-release-bot.json`. GitHub Actions cache may store disposable HTTP cache data, but it is not the source of truth.

The state file tracks:

- `schema_version`,
- source state keyed by strategy and upstream identifier,
- package name,
- upstream source kind,
- last checked version/revision,
- last successful generated artifact metadata,
- ETag or cache validators when available,
- last failure reason code,
- rate-limit reset time when provided,
- exponential backoff state when no upstream reset time is provided,
- quarantine records keyed by package name.

Quarantine records track:

```json
{
  "schema_version": 2,
  "sources": {},
  "packages": {
    "ripgrep": {
      "source_key": "github_releases:BurntSushi/ripgrep",
      "last_checked_at": "2026-05-04T12:00:00Z",
      "latest_version": "15.2.0",
      "last_successful_version": "15.1.0",
      "backoff_until": null,
      "last_failure": null
    }
  },
  "quarantine": {
    "zig": {
      "reason_code": "metadata-malformed",
      "detail": "missing artifact url for x86_64-unknown-linux-gnu",
      "first_seen_at": "2026-05-04T12:00:00Z",
      "last_seen_at": "2026-05-04T12:00:00Z",
      "attempted_version": "0.16.0",
      "last_good_version": "0.15.2"
    }
  }
}
```

State must not contain signing secrets, GitHub tokens, upstream credentials, private URLs, or raw response bodies that may include secrets.

## CLI/UX Contracts

Registry automation commands emit deterministic machine-readable summaries. Existing lines may be extended, but automation consumers must be able to count updated, up-to-date, quarantined, transient-failed, and skipped packages.

Example output:

```text
registry_update package=ripgrep status=updated from=14.1.0 to=14.1.1 source=github-release
registry_update package=fd status=up-to-date version=10.2.0 source=github-release
registry_update package=zig status=quarantined reason=metadata-malformed attempted=0.16.0 last_good=0.15.2
registry_update package=node status=skipped reason=rate-limited reset_at=2026-05-04T18:00:00Z
registry_update_summary updated=12 up_to_date=64 quarantined=1 transient_failed=2 skipped=3
```

Bot PR summaries include:

- changed packages,
- generated package/release file counts,
- state-only changes,
- quarantine additions/updates/clears,
- rate-limited/backoff packages,
- validation command results.

Crosspack client warnings for package-level poison should be reason-coded and additive. Plain warning lines go to stderr and use stable escaped fields:

```text
warning: registry_package_skipped package=zig reason=package-metadata-invalid source=core detail="failed parsing package template"
```

Warning detail values must be shell-style escaped or quoted so spaces do not break field parsing. These warnings must not change existing machine-oriented install/update transaction line shapes such as `transaction_preview`, `transaction_summary`, `risk_flags`, `change_*`, or `update summary: updated=<n> up-to-date=<n> failed=<n>`.

Root repo release workflows continue to treat registry submodule-only bumps as non-release changes.

## Failure Modes

- Upstream API 403/rate-limited: record per-package/source reason and reset/backoff; skip that package for the run; continue unrelated packages.
- Upstream asset missing or selector mismatch: quarantine that package/version; continue unrelated packages.
- Generated package/release TOML malformed: quarantine that package; do not include the malformed generated file in the PR.
- Generated diff includes unrelated paths: fail the bot run.
- Signature sidecars missing in bot PR: allowed before merge because merge-time signing owns sidecars.
- Merge-time signature generation failure: fail the signing workflow and leave registry `main` visibly unhealthy until corrected.
- Smoke install transient download failure: record warning/backoff; do not poison unrelated package updates.
- Source-level trust failure in source sync or Crosspack client: fail closed for that source.
- Signed package-level poison in source sync: mark the source snapshot ready, record diagnostics, and let broad client reads skip the poisoned package.
- Package-level poison in Crosspack client: skip or isolate that package and emit a warning where the command can proceed without selecting that exact invalid package.
- User explicitly installs a quarantined or malformed package: fail that package request with a direct diagnostic.

## Testing Requirements

- Source strategy tests with fixture upstream responses.
- State schema read/write and migration tests.
- Rate-limit reset and exponential fallback backoff tests.
- Quarantine add/update/clear tests.
- Rolling PR branch regeneration tests that prove open PRs accumulate updates from current `main`.
- Proportional quality gate tests for changed manifests only.
- Deterministic generation tests.
- Signature sidecar validation tests for post-merge state.
- End-to-end automation dry-run tests that do not require secrets.
- Crosspack registry source sync tests proving signed malformed package records do not block ready snapshots.
- Crosspack registry reader tests proving malformed package records do not block unrelated package names/search/provider/dependency/install resolution.
- Source-level trust tests proving missing keys, bad fingerprints, no ready snapshots, and missing/invalid signatures still fail closed during source sync and client reads.

## Rollout Plan

1. Extend registry bot state schema to version 2 with package state, backoff, and quarantine while migrating version 1 state.
2. Add package-scoped result accounting to `upstream-release-bot.py`.
3. Add quarantine write/clear behavior around generation and validation.
4. Change PR automation to one rolling branch regenerated from current `main`.
5. Update registry bot workflow defaults to use the rolling branch and broad automerge.
6. Split `source_sync.rs` validation so signed package poison does not block ready snapshots.
7. Harden `crosspack-registry` package reads so package-level poison does not block unrelated packages.
8. Preserve source-level trust fail-closed tests.
9. Update agent and public docs that describe registry automation behavior.

## Decisions

- Durable state lives in `registry/state/`; CI cache is disposable acceleration only.
- Core registry automation is intentionally fully autonomous and unsafe.
- All generated package updates may automerge after required checks pass.
- Signing remains merge-time through the existing signing workflow.
- Malformed metadata blocks/quarantines only the affected package.
- Source snapshots may become ready with signed malformed package records; unsigned or unverifiable records remain fatal source-level trust failures.
- Quarantine is durable state and applies to both automation and Crosspack client reads.
- Open bot PRs accumulate updates; scheduled runs update the existing rolling PR.
- Each run regenerates the bot branch from current `main` and force-updates only the bot-owned branch.
- Rate limits use per-package/source backoff.
- Valid regenerated metadata plus package validation clears quarantine.
- Registry `main` push already bumps the root submodule; keep that path non-release.
- Spec and implementation plan stay under `.agents/`.
