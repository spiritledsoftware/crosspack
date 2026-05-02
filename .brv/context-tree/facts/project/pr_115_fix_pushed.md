---
title: PR 115 Fix Pushed
summary: 'PR #115 received commit fce7277 to gate PTY test dependency on Unix; checks were rerunning and .brv context files were left uncommitted.'
tags: []
related: [facts/project/context.md]
keywords: []
createdAt: '2026-05-02T20:07:41.673Z'
updatedAt: '2026-05-02T20:07:41.673Z'
---
## Reason
Record the pushed fix and PR status for the Windows CI issue

## Raw Concept
**Task:**
Document the push of the Windows CI fix to PR #115

**Changes:**
- Pushed commit fce7277 to the PR
- Gated PTY test dependency on Unix
- Left generated .brv context files out of the fix commit

**Files:**
- .brv/context-tree/
- PR #115

**Flow:**
commit fix -> push branch -> checks rerun -> note unrelated local context files

**Timestamp:** 2026-05-02T20:07:35.779Z

**Author:** Ian

## Narrative
### Structure
A small follow-up fix was pushed directly to PR #115 on branch opencode/kimaki-tui-polish.

### Dependencies
The fix targeted the Windows CI failure and did not include generated .brv context files.

### Highlights
The branch was pushed and GitHub checks were rerunning immediately after the commit.

### Examples
Commit: fce7277 fix(cli): gate PTY test dependency on Unix

## Facts
- **pr_115_commit**: Commit fce7277 was pushed to PR #115 [project]
- **pr_115_commit_message**: The commit message was "fix(cli): gate PTY test dependency on Unix" [project]
- **pr_115_branch**: The branch was opencode/kimaki-tui-polish [project]
- **pr_115_repo**: PR #115 was on GitHub at spiritledsoftware/crosspack [project]
- **pr_115_checks_status**: Current PR checks were rerunning after the push [project]
- **brv_context_files_status**: Generated .brv context files were left uncommitted because they were unrelated to the Windows CI failure [project]
