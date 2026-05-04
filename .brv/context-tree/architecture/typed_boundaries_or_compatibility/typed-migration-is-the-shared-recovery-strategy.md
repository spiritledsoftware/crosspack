---
confidence: 0.97
sources:
  - facts/_index.md
  - project/_index.md
  - project/_index.md
synthesized_at: '2026-05-03T02:26:46.685Z'
type: synthesis
---

# Typed migration is the shared recovery strategy

Across both domains, the recurring design move is to introduce typed, identity-aware, or durable internal mechanisms while continuing to accept legacy state and formats until migration completes. In the project domain this shows up in installer state, rollback, and transaction recovery hardening; in the facts domain it is elevated to the cross-cutting architectural principle that compatibility is preserved through typed migration rather than parallel ad hoc behavior.

## Evidence

- **facts**: The knowledge entries converge on one recurring architectural principle: compatibility is maintained through typed migration, not parallel ad hoc behavior.
- **project**: Transaction recovery hardening routes metadata writes, journal updates, and active-marker cleanup through durable helpers while preserving legacy metadata behavior and conflict errors.
- **project**: Installer state work moves from package-name-centric logic toward identity-aware state while retaining legacy receipt visibility for rollback and compatibility.
