---
title: Working Directory Switch and Blocked CLI Test
summary: The repo switched to tui-rework, the old transaction-recovery-hardening worktree is off-limits, and a CLI identity snapshot test was interrupted while waiting on a Cargo artifact lock.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T11:08:30.589Z'
updatedAt: '2026-05-03T11:08:30.590Z'
---
## Reason
Preserve lasting operational facts from the cwd switch and blocked test report

## Raw Concept
**Task:**
Document the workspace switch and blocked test outcome

**Changes:**
- Changed the active working directory to tui-rework
- Recorded that transaction-recovery-hardening is not to be touched
- Captured the interrupted identity snapshot test outcome

**Files:**
- crates/crosspack-cli/src/tests.rs

**Flow:**
cwd switch -> scope restriction -> cargo test -> artifact lock wait -> Ctrl+C interruption

**Timestamp:** 2026-05-03T11:08:20.038Z

**Author:** user conversation log

## Narrative
### Structure
This context is a short operational record about a workspace change and a blocked test run.

### Dependencies
The blocked command depended on Cargo artifact availability and the currently active worktree scope.

### Highlights
The important lasting details are the new cwd, the forbidden previous worktree, and the interrupted CLI identity snapshot test.

### Examples
The recorded command was: cargo test -p crosspack-cli identity_snapshot_restores_identity_scoped_payload_task_8_inventory_gap -- --test-threads=1.

## Facts
- **working_directory**: The working directory was changed to /home/ianpascoe/.kimaki/worktrees/060b9059/tui-rework. [project]
- **restricted_worktree**: The previous folder /home/ianpascoe/.kimaki/worktrees/060b9059/transaction-recovery-hardening must not be touched. [project]
- **git_branch**: The current git branch is opencode/kimaki-tui-rework. [project]
- **test_outcome**: A focused CLI identity snapshot test was run and interrupted with Ctrl+C. [project]
- **test_command**: The interrupted test used cargo test -p crosspack-cli identity_snapshot_restores_identity_scoped_payload_task_8_inventory_gap -- --test-threads=1. [project]
- **cargo_artifact_lock**: The test process was blocked on a Cargo artifact lock before interruption. [project]
- **task_8_test_file**: The task work mentioned a Task 8 identity rollback test in crates/crosspack-cli/src/tests.rs in the transaction-recovery-hardening worktree. [project]
- **scope_constraint**: No further Task 7-8 work could continue under the current scope constraint. [project]
