---
confidence: 0.95
sources:
  - architecture/_index.md
  - architecture/_index.md
  - architecture/_index.md
  - facts/_index.md
  - facts/_index.md
synthesized_at: '2026-04-29T17:24:21.436Z'
type: synthesis
---

# Typed boundaries as the organizing principle for deterministic automation

Across the architecture and project-guidance domains, the repo is consistently moving authority out of ad hoc or shared-scope logic and into typed, explicit boundaries so behavior becomes deterministic, previewable, and easier to validate. In architecture this appears in the shift to typed command/service modules, typed install plans, typed transaction status, and typed host integrations; in facts/project it appears in workflow gating, release-path filtering, and separating non-user-facing changes from release behavior.

## Evidence

- **architecture**: Crosspack is moving away from ad hoc CLI and receipt recomputation toward typed, plan-driven boundaries with deterministic preview/apply behavior.
- **architecture**: Typed InstallPlan data should become the source of truth for install/apply behavior, preview output, and replacement handling.
- **architecture**: TransactionMetadata.status moved from String to TransactionStatus while preserving compatibility with legacy tokens.
- **facts**: CI, Release Please, prerelease artifacts, and dependency review rely on allow-list path filters, and registry submodule bumps are explicitly treated as non-release changes.
- **facts**: Workflow-only PRs get actionlint via a dedicated Workflow Lint workflow, while docs paths were removed from expensive Rust CI and release filters.
