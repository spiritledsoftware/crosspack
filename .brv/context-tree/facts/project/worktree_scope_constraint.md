---
title: Worktree Scope Constraint
summary: Edits must remain within the current worktree /home/ianpascoe/.kimaki/worktrees/060b9059/transaction-recovery-hardening; the previous checkout /home/ianpascoe/code/crosspack must not be touched.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T09:08:29.622Z'
updatedAt: '2026-05-03T09:08:29.622Z'
---
## Reason
Record active workspace constraint from the conversation

## Raw Concept
**Task:**
Preserve the active git worktree editing constraint for the transaction-recovery-hardening session

**Changes:**
- Switched editing scope to the new worktree
- Prohibited touching the previous checkout

**Files:**
- /home/ianpascoe/.kimaki/worktrees/060b9059/transaction-recovery-hardening
- /home/ianpascoe/code/crosspack

**Flow:**
detect cwd change -> restrict edits to new worktree -> run validation in the same worktree

**Timestamp:** 2026-05-03T09:08:23.700Z

**Author:** user

## Narrative
### Structure
This is an operational constraint for the active session and applies to all subsequent file reads, writes, edits, and validation commands.

### Dependencies
Depends on the current git worktree context and must be honored to avoid overwriting unrelated changes in another checkout.

### Highlights
The user explicitly expects all operations to stay inside the new worktree for the transaction-recovery-hardening branch.

### Rules
You MUST operate inside the new worktree from now on.
You MUST NOT read, write, or edit any files under the previous folder /home/ianpascoe/code/crosspack.
Run all checks (tests, builds, lint) inside the new worktree.

## Facts
- **active_worktree_path**: The active worktree path is /home/ianpascoe/.kimaki/worktrees/060b9059/transaction-recovery-hardening. [project]
- **forbidden_checkout_path**: The previous checkout /home/ianpascoe/code/crosspack must not be read, written, or edited. [project]
- **check_execution_scope**: Run checks such as tests, builds, and lint inside the new worktree. [project]
