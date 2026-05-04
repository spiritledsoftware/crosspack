---
title: Render formatter test failure
summary: Render formatter tests failed with exit code 101; rerun hint is `-p crosspack-cli --bin crosspack`.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T11:21:11.048Z'
updatedAt: '2026-05-03T11:21:11.048Z'
---
## Reason
Capture lasting outcome from failed render formatter test run

## Raw Concept
**Task:**
Document the outcome of the render formatter test run

**Changes:**
- Test run exited with code 101
- Failure output suggested rerunning the crosspack CLI binary

**Files:**
- crates/crosspack-cli/src
- crosspack-cli binary

**Flow:**
test execution -> failure -> rerun hint emitted

**Timestamp:** 2026-05-03T11:21:07.219Z

## Narrative
### Structure
This records a failed formatter test execution rather than a code change.

### Dependencies
The failure message references the crosspack CLI binary for reruns.

### Highlights
The test did not complete successfully and returned exit code 101.

## Facts
- **render_formatter_tests_result**: Run render formatter tests failed with exit code 101. [project]
- **render_formatter_tests_rerun_hint**: The rerun command suggested by the test output is `-p crosspack-cli --bin crosspack`. [project]
