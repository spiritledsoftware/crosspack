---
title: Rollback Corrupt Marker Test Failure
summary: Focused rollback corrupt marker test run failed with exit code 101; rerun hint was `-p crosspack-cli --bin cpk`.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T09:15:45.651Z'
updatedAt: '2026-05-03T09:15:45.651Z'
---
## Reason
Record the failed focused test run and rerun hint

## Raw Concept
**Task:**
Capture the outcome of the rollback corrupt marker focused test run

**Changes:**
- Recorded a failed sequential test run for rollback corrupt marker coverage

**Flow:**
test run -> failure -> rerun hint emitted

**Timestamp:** 2026-05-03T09:15:41.881Z

## Narrative
### Structure
The recorded outcome is a terminal test execution report with exit status and rerun guidance.

### Dependencies
The rerun hint references the crosspack-cli package binary target.

### Highlights
The focused test failed quickly with exit code 101 and suggested rerunning the cpk binary test target.

## Facts
- **rollback_corrupt_marker_test_status**: A focused rollback corrupt marker test run failed. [project]
- **rollback_corrupt_marker_test_exit_code**: The test exited with code 101. [project]
- **rollback_corrupt_marker_test_rerun_hint**: The rerun hint was `-p crosspack-cli --bin cpk`. [project]
