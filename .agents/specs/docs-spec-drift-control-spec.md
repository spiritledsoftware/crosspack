# Docs Spec Drift Control Spec

**Status:** roadmap, non-GA
**Last updated:** 2026-04-29

## Problem

Crosspack has shipped docs, roadmap specs, agent plans, and implementation behavior moving at different speeds. Without explicit drift control, roadmap language can read like shipped behavior, shipped docs can lag implementation, and future agents can plan from stale assumptions.

## Goals

- Keep shipped docs accurate for current behavior.
- Keep roadmap specs clearly labeled as non-GA until implemented.
- Make implementation PRs update docs when behavior changes.
- Protect machine-oriented CLI output contracts from accidental docs/test drift.
- Give agents a clear path from spec to implementation plan to shipped docs.

## Non-Goals

- Do not require every internal note to become shipped documentation.
- Do not block small code fixes on large documentation rewrites.
- Do not treat `.agents/*` planning files as user-facing product docs.
- Do not make docs generation replace human review of behavioral claims.

## Current State

- `docs/architecture.md` and `docs/install-flow.md` are the best shipped-behavior references.
- `docs/*-spec.md` can be roadmap/non-GA.
- `.agents/plans/` holds agent planning/design docs.
- `.agents/specs/` now holds long-horizon roadmap specs.
- Release checklist already asks to confirm v0.4/v0.5 specs are labeled as roadmap drafts.

## Target Behavior

Documentation layers:

- Shipped behavior: `README.md`, `docs/architecture.md`, `docs/install-flow.md`, command help.
- Roadmap specs: `docs/*-spec.md` and `.agents/specs/*` when marked non-GA.
- Implementation plans: `.agents/plans/*`.
- Durable memory/context: `.brv/context-tree/*`.

Rule: any behavior-changing PR must update either shipped docs or explicitly confirm no shipped docs changed.

## Architecture

```text
roadmap spec
     |
     v
implementation plan
     |
     v
code + tests + contract output
     |
     v
shipped docs update
     |
     v
release checklist verification
```

Docs drift checks should be lightweight and targeted rather than a broad docs bureaucracy.

## Data/State Model

Spec frontmatter or header should include:

- status,
- shipped/non-GA classification,
- related shipped docs,
- last updated date.

Behavior-sensitive docs should reference tests or command contracts where possible.

## CLI/UX Contracts

Machine-oriented output tokens documented in specs or shipped docs must have tests:

- `transaction_preview`
- `transaction_summary`
- `risk_flags`
- `change_add`
- `change_remove`
- `change_replace`
- `change_transition`
- `update summary: updated=<n> up-to-date=<n> failed=<n>`

If a token changes, the PR must include coordinated code, tests, docs, and release notes.

## Failure Modes

- Roadmap spec lacks non-GA label: release checklist catches it.
- Shipped docs claim unimplemented behavior: docs review blocks PR.
- Code changes behavior without docs update: PR checklist or CI docs lint flags it.
- Contract output changes without tests: CLI tests fail or reviewer blocks.
- Agent plans cite stale spec language: spec index points to current source of truth.

## Testing Requirements

- Markdown lint or targeted script checks for required status labels in roadmap specs.
- Contract tests for documented machine output tokens.
- Release checklist validation for roadmap-vs-shipped wording.
- Link checks for spec index and shipped docs references.
- Optional docs grep tests preventing phrases like "fully implemented" in non-GA specs.

## Rollout Plan

1. Add consistent headers to long-horizon specs.
2. Add an index for `.agents/specs`.
3. Update PR/release checklist language for docs drift checks.
4. Add lightweight script checks for non-GA labels and broken links.
5. Tie behavior-changing PR templates to docs update confirmation.

## Open Questions

- Should `.agents/specs` be committed long term, or eventually promoted into `docs/` when ready?
- Should docs drift checks run in CI for all PRs or only docs/product-impacting paths?
- Should release notes mention roadmap spec changes, or only shipped behavior changes?
