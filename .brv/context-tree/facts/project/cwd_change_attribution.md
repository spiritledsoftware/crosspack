---
title: Cwd Change Attribution
summary: Cwd changes were attributed to Kimaki/session state and synthetic notices, not normal bash or PTY shell cd commands.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T12:05:12.848Z'
updatedAt: '2026-05-03T12:05:12.848Z'
---
## Reason
Capture lasting evidence about cwd changes being imposed by session metadata rather than shell commands

## Raw Concept
**Task:**
Document the observed cwd change behavior in the transaction-recovery-hardening workspace

**Changes:**
- Observed cwd changes are imposed by session/system-message level state
- Normal bash commands did not change cwd via cd
- PTY exits can be followed by injected cwd-change reminders pointing to another worktree

**Flow:**
PTY or shell command runs -> Kimaki/session injects cwd metadata notice -> conversation appears to switch worktree

**Timestamp:** 2026-05-03T12:05:07.092Z

## Narrative
### Structure
The report distinguishes shell-level behavior from session-level metadata updates and attributes the switch to Kimaki rather than the shell process.

### Dependencies
Relies on tool-call workdir settings, synthetic notices about worktree changes, and PTY exit timing.

### Highlights
Likely bug: cwd metadata is being updated from the wrong thread/worktree after PTY/session events.

## Facts
- **cwd_switch_attribution**: The cwd switch arrives as a synthetic user/system notice, not from normal bash commands. [project]
- **pty_cwd_behavior**: PTY commands did not cd; they were spawned with workdir set. [project]
- **cwd_metadata_bug**: Kimaki may be associating PTY/session events with the wrong active worktree/thread and updating conversation cwd metadata from another thread/worktree. [project]
