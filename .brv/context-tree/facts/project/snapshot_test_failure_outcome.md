---
title: Snapshot Test Failure Outcome
summary: Stable named snapshot generation failed in crosspack-cli with exit code 101 and test rerun hint.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T11:40:46.801Z'
updatedAt: '2026-05-03T11:40:46.801Z'
---
## Reason
Record durable outcome from failed snapshot generation test run

## Raw Concept
**Task:**
Document the failed stable named snapshots test outcome for crosspack-cli.

**Changes:**
- Captured failure exit code and rerun hint from the terminated process

**Flow:**
test run -> pty exit -> failure reported -> rerun hint provided

**Timestamp:** 2026-05-03T11:40:42.074Z

## Narrative
### Structure
The run ended as a failed pty execution during snapshot generation testing.

### Highlights
The failure produced exit code 101 and explicitly pointed to the crosspack-cli binary for reruns.

## Facts
- **snapshot_generation_status**: The stable named snapshots generation run failed. [project]
- **failed_process**: The failing process was pty_3ba1745d with description "Generate stable named snapshots". [project]
- **exit_code**: The exit code was 101. [project]
- **timeout_seconds**: The timeout was 900 seconds. [project]
- **rerun_hint**: The error message suggested rerunning with `-p crosspack-cli --bin crosspack`. [project]
