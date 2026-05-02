---
title: Windows clippy rexpect gating
summary: Windows clippy failed because rexpect 0.6.4 was built for --all-targets; the fix moved rexpect to cfg(unix) dev-dependencies in crosspack-cli.
tags: []
related: []
keywords: []
createdAt: '2026-05-02T20:05:38.171Z'
updatedAt: '2026-05-02T20:05:38.171Z'
---
## Reason
Persist the Windows PR check failure root cause and fix

## Raw Concept
**Task:**
Document the Windows PR check failure and local fix for crosspack-cli.

**Changes:**
- Identified rexpect 0.6.4 as the Windows clippy failure source
- Moved rexpect to Unix-only dev-dependencies
- Confirmed PTY test was already cfg(unix)
- Verified formatting, clippy, and tests locally

**Files:**
- crates/crosspack-cli/Cargo.toml

**Flow:**
Windows PR checks fail -> inspect clippy logs -> identify rexpect as the failing dev-dependency -> gate rexpect behind cfg(unix) -> rerun fmt/clippy/tests

**Timestamp:** 2026-05-02T20:05:31.530Z

**Author:** Ian

## Narrative
### Structure
The issue is a platform-specific dev-dependency problem in crosspack-cli rather than a bug in the PTY test itself. Windows clippy with --all-targets attempted to compile rexpect, which is Unix-only in practice.

### Dependencies
Depends on Cargo target-specific dev-dependencies and the existing Unix cfg on the PTY test.

### Highlights
The remedy is to move rexpect into target.'cfg(unix)'.dev-dependencies so Windows builds stop trying to compile rexpect.

### Examples
Changed crates/crosspack-cli/Cargo.toml and verified with cargo fmt --all --check, cargo clippy -p crosspack-cli --all-targets -- -D warnings, and cargo test -p crosspack-cli.

## Facts
- **windows_clippy_rexpect_failure**: Windows PR checks were failing because cargo clippy --all-targets built the dev-dependency rexpect 0.6.4 on Windows, but rexpect does not compile on Windows. [project]
- **rexpect_unix_target_dependency**: The fix moved rexpect.workspace = true from normal [dev-dependencies] to [target.'cfg(unix)'.dev-dependencies] in crates/crosspack-cli/Cargo.toml. [project]
- **pty_test_platform_gating**: The PTY-only test was already cfg(unix), so the missing piece was preventing Cargo from building rexpect during Windows --all-targets clippy. [project]
- **local_verification_commands**: Local verification passed for cargo fmt --all --check, cargo clippy -p crosspack-cli --all-targets -- -D warnings, and cargo test -p crosspack-cli. [project]
- **fix_persistence_status**: The fix was not pushed yet and remained only as a local working-tree change. [project]
