---
title: Durability and rollback are correctness boundaries
summary: Task 1-4 policy is best-effort for unsupported/platform-specific directory sync failures; current propagation of all open/sync errors would violate the plan/spec.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T09:25:58.230Z'
updatedAt: '2026-05-03T09:25:58.230Z'
---
## Reason
Record arbitration outcome for Tasks 1-4 sync_directory policy

## Raw Concept
**Task:**
Arbitrate the durability policy for durable.rs::sync_directory in the transaction recovery v0.5 hardening work

**Changes:**
- Determined the policy should be best-effort for unsupported/platform-specific directory sync failures
- Determined mandatory propagation of all open/sync errors is not the desired policy for Tasks 1-4

**Flow:**
Review plan/spec -> compare task scope and implementation behavior -> determine compliance

**Timestamp:** 2026-05-03T09:25:53.908Z

## Narrative
### Structure
This decision applies to the transaction recovery hardening scope for Tasks 1-4 and specifically the directory sync step in durable.rs.

### Dependencies
The conclusion is grounded in the plan/spec language that limits directory sync to supported environments and allows best-effort handling for unsupported cases.

### Highlights
Best-effort handling is the intended policy; mandatory failure propagation would overconstrain the behavior and conflict with the spec intent.

## Facts
- **sync_directory_policy**: The plan/spec for transaction recovery hardening treats unsupported or platform-specific directory sync failures as best-effort rather than mandatory failures. [project]
- **implementation_compliance**: Current best-effort implementation does not violate the plan/spec for Tasks 1-4. [project]
