---
title: Worktree Scope Restriction
summary: The active worktree switched to tui-rework, and transaction-recovery-hardening is now out of scope for edits and reads.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T11:08:25.445Z'
updatedAt: '2026-05-03T11:08:25.445Z'
---
## Reason
Preserve the active scope restriction and blocked task outcome

## Raw Concept
**Task:**
Document the enforced worktree scope change during a blocked task handoff

**Changes:**
- Scope switched from transaction-recovery-hardening to tui-rework
- Further access to the old worktree was explicitly forbidden

**Flow:**
cwd switch -> scope restriction -> blocked continuation in old worktree

**Timestamp:** 2026-05-03T11:08:21.263Z

## Narrative
### Structure
The session context now belongs to the tui-rework checkout, while the previous transaction-recovery-hardening checkout is off-limits.

### Dependencies
Any continuation must respect the active cwd and avoid the old worktree entirely.

### Highlights
A prior Task 7-8 effort was interrupted and cannot be resumed in the restricted checkout.

## Facts
- **current_worktree**: The working directory changed to /home/ianpascoe/.kimaki/worktrees/060b9059/tui-rework. [project]
- **restricted_worktree**: The previous worktree /home/ianpascoe/.kimaki/worktrees/060b9059/transaction-recovery-hardening must not be read, written, or edited. [project]
- **current_branch**: The branch for the active worktree is opencode/kimaki-tui-rework. [project]
