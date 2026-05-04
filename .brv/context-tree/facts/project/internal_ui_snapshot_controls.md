---
title: Internal UI Snapshot Controls
summary: Deterministic terminal capture should use internal env controls first; public --snapshot is discouraged and hidden --dump-ui-state is only a fallback.
tags: []
related: []
keywords: []
createdAt: '2026-05-03T10:00:47.107Z'
updatedAt: '2026-05-03T10:00:47.107Z'
---
## Reason
Document deterministic terminal capture controls for development and testing

## Raw Concept
**Task:**
Document development setup guidance for deterministic terminal snapshot testing

**Changes:**
- Recommended internal env controls for snapshot determinism
- Discouraged public --snapshot CLI flag
- Allowed hidden --dump-ui-state only as fallback

**Flow:**
set internal snapshot env vars -> render deterministic UI -> capture snapshots; if insufficient, use hidden state dump flag

**Timestamp:** 2026-05-03T10:00:42.058Z

**Author:** assistant

## Narrative
### Structure
The guidance prioritizes deterministic renderer controls through environment variables rather than public CLI flags.

### Dependencies
Snapshot generation depends on stable terminal width and color settings for repeatable output.

### Highlights
Snapshots should be generated without relying entirely on real PTYs. Public CLI surface area should stay minimal unless env-driven tests prove insufficient.

## Facts
- **internal_ui_snapshot**: Use CROSSPACK_INTERNAL_UI_SNAPSHOT=1 for deterministic UI snapshot mode. [project]
- **internal_term_width**: Use CROSSPACK_INTERNAL_TERM_WIDTH=<cols> to control terminal width in snapshot tests. [project]
- **internal_no_color**: Use CROSSPACK_INTERNAL_NO_COLOR=1 to disable color in deterministic terminal captures. [project]
- **public_snapshot_flag**: Avoid a public --snapshot flag because it creates long-term CLI contract burden. [project]
- **dump_ui_state_flag**: Add a hidden --dump-ui-state flag only if env-driven tests are not enough. [project]
