---
confidence: 0.98
sources:
  - architecture/_index.md
  - architecture/_index.md
  - facts/_index.md
synthesized_at: '2026-05-02T00:39:42.866Z'
type: synthesis
---

# Typed migration is the shared compatibility strategy across architecture and facts

Both domains describe the same underlying transition pattern: replace ad hoc or legacy behavior with typed internal models while preserving compatibility through adapters, fallback paths, and legacy reads. In architecture, this appears in typed transaction status plumbing, identity-keyed installed state, and install-plan source-of-truth rules; in facts, it is elevated as the explicit rule that compatibility is preserved through typed migration rather than parallel ad hoc behavior.

## Evidence

- **architecture**: TransactionMetadata.status moved from String to TransactionStatus, with strings parsed into typed statuses internally and serialized back to existing tokens.
- **architecture**: Installed identity/profile state moved to identity-keyed storage while legacy name-keyed reads remain supported for migration and rollback visibility.
- **facts**: The compatibility-is-preserved-through-typed-migration-not-parallel-ad-hoc-behavior topic generalizes the rule that new typed models replace older behavior internally while legacy inputs and paths remain accepted during transition.
