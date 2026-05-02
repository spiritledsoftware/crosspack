---
confidence: 0.95
sources:
  - architecture/_index.md
  - facts/_index.md
synthesized_at: '2026-05-01T09:19:39.169Z'
type: synthesis
---

# Identity-aware state is the bridge between installer correctness and legacy compatibility

A cross-cutting theme is migrating state handling from package/name-keyed assumptions to identity-aware storage while preserving legacy fallbacks, so concurrent installs and uninstall/rollback behavior become safer without breaking older data.

## Evidence

- **architecture**: The installed identity/profile and installer state model entries introduce InstalledPackageIdentity, selector-aware matching, and storage under pkgs/by-identity/<identity-key>/<version>/, while legacy reads still fall back across new identity, legacy identity, and name-keyed paths.
- **facts**: Project notes record identity uninstall and rollback compatibility fixes, legacy receipt handling, and service-state dual writes; they also note that same-name concurrent installs can coexist under identity-keyed storage.
