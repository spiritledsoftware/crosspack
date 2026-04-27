---
title: Registry Source Strategy Expansion
summary: Registry now supports automatable upstream source strategies for Go, Rustup static archives, Zig download index, and python-build-standalone; manual source detour removed.
tags: []
related: []
keywords: []
createdAt: '2026-04-27T10:40:04.375Z'
updatedAt: '2026-04-27T10:40:04.375Z'
---
## Reason
Record lasting registry automation changes and validation outcomes

## Raw Concept
**Task:**
Document registry source automation and validation improvements

**Changes:**
- Replaced manual source detour with automatable source kinds
- Added source strategies for Go dist metadata, python-build-standalone, Rustup static archives, and Zig download index
- Added checksum and asset strategy support for upstream indexes and .sha256 sidecars
- Updated upstream release bot planning and generation
- Removed the manual provider detour

**Flow:**
upstream source discovery -> normalize release metadata -> generate registry packages -> validate source coverage -> dry-run release bot -> smoke tests

**Timestamp:** 2026-04-27T10:39:58Z

**Patterns:**
- `item["version"]` - Version-key access used during Zig release sorting; normalization was needed when nested version was absent.

## Narrative
### Structure
Registry tooling now treats common language/runtime sources as first-class generated upstream strategies instead of special manual cases.

### Dependencies
Relies on upstream release metadata/indexes for Go, Rustup, Zig, python-build-standalone, and GitHub release assets for bun and deno.

### Highlights
A Zig metadata normalization bug was identified and fixed by copying the semver key into each release record before sorting. All listed validation commands passed, and unrelated existing changes in .opencode and .brv were left untouched.

## Facts
- **registry_source_strategy**: Registry entries can use deterministic upstream sources and must be first-class automatable source strategies rather than manual. [project]
- **manual_source_path**: The manual source path was removed. [project]
- **source_strategies**: Added source strategies go_dist_index, python_build_standalone, rustup_static, and zig_download_index. [project]
- **validated_packages**: Validation and dry-run checks passed for bun, deno, go, python, rustup-init, and zig. [project]
