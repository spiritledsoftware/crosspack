# Crosspack Long-Horizon Specs

**Status:** roadmap workspace, non-GA
**Last updated:** 2026-04-29

This directory holds long-horizon product and architecture specs that are not yet shipped behavior. Treat these files as planning source material for future implementation plans. Shipped behavior remains documented in `docs/architecture.md` and `docs/install-flow.md`.

## Specs

1. [Installed Identity Profile Model](installed-identity-profile-model-spec.md)
   - Expands installed package identity beyond legacy package names and current target-aware documents.
   - Covers profile selectors, target/source disambiguation, and multi-profile lifecycle behavior.

2. [Dependency Policy v0.4 Follow-Through](dependency-policy-v0-4-follow-through-spec.md)
   - Completes the policy surface around `provides`, `conflicts`, and `replaces` after the first typed plan implementation.
   - Covers provider stability, registry coverage, and end-to-end policy confidence.

3. [Transaction Recovery v0.5 Hardening](transaction-recovery-v0-5-hardening-spec.md)
   - Hardens rollback and repair beyond the current typed transaction coordinator and snapshot replay.
   - Covers fsync durability, recovery policy, rollback payload completeness, and artifact signature hooks.

4. [Typed Host Integrations Expansion](typed-host-integrations-expansion-spec.md)
   - Extends the first declarative integration slice into host-facing integration adapters.
   - Covers service lifecycle, opt-in host projection, adapters, and registry validation packages.

5. [No Post-Install Scripts Policy](no-post-install-scripts-policy-spec.md)
   - Codifies Crosspack's no-arbitrary-post-install-scripts stance as a product and security policy.
   - Covers typed alternatives, registry acceptance rules, and escape-hatch boundaries.

6. [Registry Automation Maturation](registry-automation-maturation-spec.md)
   - Matures registry updates from manual package bumps toward audited automated pipelines.
   - Covers upstream source strategies, state caching, rate-limit handling, quality gates, and release separation.

7. [Docs Spec Drift Control](docs-spec-drift-control-spec.md)
   - Keeps shipped docs, roadmap specs, and implementation behavior from drifting apart.
   - Covers spec labels, contract tests, docs review gates, and release checklist integration.

## Usage Rules

- Do not treat these specs as shipped behavior until implementation, tests, and docs are merged.
- When implementation begins, write a separate implementation plan under `.agents/plans/`.
- Keep machine-oriented CLI output contracts stable unless a spec explicitly calls for a coordinated contract migration.
- Prefer adding a small spec update when a future PR intentionally changes roadmap direction.
