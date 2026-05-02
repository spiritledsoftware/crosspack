---
confidence: 0.88
sources:
  - architecture/_index.md
  - architecture/_index.md
  - facts/_index.md
synthesized_at: '2026-05-02T00:39:42.888Z'
type: synthesis
---

# Compatibility preservation is an intentional design constraint, not a temporary workaround

Both domains show compatibility handling as a deliberate part of the design rather than a side effect. In architecture, legacy state, legacy receipts, and older status tokens are preserved during transitions; in facts, the durable pattern itself is recorded as a stable project rule, which suggests future changes should continue to favor migration paths that do not break existing behavior.

## Evidence

- **architecture**: Legacy name-keyed reads remain supported for migration and rollback visibility even after identity-keyed storage was introduced.
- **architecture**: Typed transaction status plumbing preserves existing serialized tokens while using typed statuses internally.
- **facts**: The compatibility pattern is explicitly recorded as durable knowledge, emphasizing typed internal models plus compatibility adapters during transition.
