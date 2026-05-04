---
confidence: 0.91
sources:
  - project/_index.md
  - project/_index.md
  - facts/_index.md
synthesized_at: '2026-05-03T02:26:46.690Z'
type: synthesis
---

# Durability and rollback are treated as correctness boundaries, not just implementation details

The project treats durable file operations, atomic replacement, append-only journaling, and fail-closed rollback handling as part of the system’s correctness model. This fits the broader facts-domain pattern that compatibility and stability are maintained by explicit boundaries and controlled transitions, rather than by letting multiple behaviors coexist indefinitely.

## Evidence

- **project**: The installer introduced a private durable helper module to centralize write_file_atomic, append_line, remove_file_if_exists_durable, and sync_directory.
- **project**: Rollback must treat empty or corrupt active-marker state as fail-closed, not silently clean.
- **facts**: The shared architectural throughline says to prefer typed boundaries and identity-keyed state, preserve legacy compatibility only as an explicit transition path, and keep release/CI scope narrow and explicit.
