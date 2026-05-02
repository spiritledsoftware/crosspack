---
confidence: 0.92
sources:
  - architecture/_index.md
  - architecture/_index.md
  - facts/_index.md
synthesized_at: '2026-05-02T00:39:42.868Z'
type: synthesis
---

# Deterministic systems are protected by explicit source-of-truth boundaries, not recomputation from legacy views

A cross-cutting architectural rule is to make one typed artifact authoritative and prevent later stages from recomputing behavior from older representations. Architecture applies this to InstallPlan and identity-keyed state, while the facts domain reinforces the same broader compatibility principle: keep legacy access paths only as adapters, not as parallel decision-makers.

## Evidence

- **architecture**: InstallPlanPackage must be the source of truth for apply behavior; apply should fail if expected planned package membership is absent.
- **architecture**: Apply logic was identified as unsafe when it recomputed membership and install reasons from receipts/root names instead of plan data.
- **facts**: Compatibility is preserved through typed migration with compatibility adapters, not by maintaining parallel ad hoc behaviors.
