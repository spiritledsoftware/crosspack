---
title: Registry CalVer version normalization
summary: crosspack now accepts CalVer-style registry versions with zero-padded core components by normalizing them after strict SemVer parsing fails.
tags: []
related: []
keywords: []
createdAt: '2026-05-02T10:31:17.764Z'
updatedAt: '2026-05-02T10:31:17.764Z'
---
## Reason
Capture the install bootstrap fix and version parsing behavior

## Raw Concept
**Task:**
Fix install bootstrap failure caused by signed registry manifests using CalVer-style versions

**Changes:**
- Added fallback normalization for leading-zero core version components after strict SemVer parsing fails
- Added test coverage for the Helix-style registry version
- Confirmed bootstrap and search behavior after the fix

**Files:**
- crates/crosspack-core/src/manifest.rs
- crates/crosspack-core/src/tests.rs
- registry/releases/helix/25.07.1.toml

**Flow:**
crosspack update -> validate signed registry manifests -> parse version -> strict semver fails -> normalize leading-zero core components -> continue

**Timestamp:** 2026-05-02

**Author:** assistant

## Narrative
### Structure
The version parsing change lives in crosspack-core manifest deserialization, with tests updated alongside it.

### Dependencies
Depends on signed registry manifest validation and semver parsing behavior; the failure surfaced as source-metadata-invalid.

### Highlights
The bootstrap failure was traced to a CalVer-style registry release version with zero-padded components. The fix preserves strict SemVer behavior first, then applies minimal normalization only when needed.

### Examples
Helix release manifest version 25.07.1 is accepted internally as 25.7.1 after normalization.

## Facts
- **bootstrap_repro**: Bootstrap repro now succeeds after running crosspack update to refresh the source. [project]
- **helix_manifest_version**: The Helix registry manifest version 25.07.1 was rejected by strict semver parsing because of a leading zero in the core version component. [project]
- **registry_version_deserialization**: Version deserialization now tries strict SemVer first and then normalizes leading-zero core version components only. [project]
- **calver_normalization_example**: The normalized internal example is 25.07.1 -> 25.7.1. [project]
- **verification_commands**: The fix was verified with cargo fmt --all --check, cargo test -p crosspack-core, cargo test -p crosspack-registry, and cargo clippy -p crosspack-core -p crosspack-registry --all-targets -- -D warnings. [project]
- **local_bootstrap_repro**: Local bootstrap repro using registry add and crosspack update now succeeds. [project]
- **helix_search_result**: crosspack search helix now returns 25.7.1. [project]
