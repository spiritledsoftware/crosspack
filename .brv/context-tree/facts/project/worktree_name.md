---
title: Worktree Name
summary: The active worktree is opencode/kimaki-transaction-recovery-hardening.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T09:25:56.202Z'
updatedAt: '2026-05-03T09:27:08.982Z'
---
## Reason
Capture the active worktree name from the conversation for durable project context

## Raw Concept
**Task:**
Record the active worktree name for the current session

**Changes:**
- Captured the worktree identifier from the user message

**Flow:**
user message -> extract worktree name -> store as durable project fact

**Timestamp:** 2026-05-03T09:27:05.065Z

**Author:** Ian

## Narrative
### Structure
A single project fact under facts/project that records the current worktree identifier.

### Highlights
Useful for later session continuity and workspace identification.

## Facts
- **worktree_name**: The active worktree is opencode/kimaki-transaction-recovery-hardening. [project]
