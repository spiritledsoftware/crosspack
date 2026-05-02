---
confidence: 0.9
sources:
  - architecture/_index.md
  - facts/_index.md
synthesized_at: '2026-05-01T09:19:39.171Z'
type: synthesis
---

# Compatibility is preserved through typed migration, not parallel ad hoc behavior

The knowledge base repeatedly shows a migration pattern where new typed models are introduced internally while external tokens, legacy paths, or old workflows remain accepted for compatibility until the transition is complete.

## Evidence

- **architecture**: TransactionMetadata.status moved from String to TransactionStatus, but incoming strings are still parsed and serialized back to existing tokens; the same theme appears in the installed identity/profile model’s fallback reads across new and legacy state paths.
- **facts**: Project follow-through notes emphasize shipped slices versus long-horizon specs, including partially implemented identity/profile, dependency policy, recovery hardening, typed host integrations, and docs drift control, which indicates staged migration rather than abrupt replacement.
