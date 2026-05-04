---
confidence: 0.92
sources:
  - architecture/_index.md
  - facts/_index.md
  - project/_index.md
synthesized_at: '2026-05-03T12:13:37.340Z'
type: synthesis
---

# Identity-aware state is the practical bridge between compatibility and correctness

Installer and rollback behavior depends on moving from name-keyed assumptions to identity-aware storage without breaking older receipts or service state. The same idea is reflected in project facts about uninstall/rollback compatibility and in architecture notes about typed boundaries preserving legacy reads while tightening internal correctness.

## Evidence

- **architecture**: The architecture summary says identity-aware state replaces name-keyed assumptions, with selector-aware matching, storage under identity-keyed paths, and fallback reads across new identity, legacy identity, and name-keyed paths.
- **facts**: The facts summary says the installed identity/profile model preserves legacy compatibility while making rollback and uninstall behavior correct, including fallback reads across new and legacy state paths.
- **project**: The project summary records installer receipt and rollback hardening work, including legacy receipt compatibility and deduplication of overlapping receipts.
