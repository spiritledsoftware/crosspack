---
title: Exploration Tool Usage Preference
summary: Ian prefers explore for read-only review tasks, while general is for implementation/fix work; if needed, general can also be used for reviews with explicit no-edit instructions.
tags: []
related: []
keywords: []
createdAt: '2026-05-02T18:16:58.872Z'
updatedAt: '2026-05-02T18:16:58.872Z'
---
## Reason
Record the clarified choice between explore and general for future review and implementation tasks

## Raw Concept
**Task:**
Document the preferred tool choice for review-only versus implementation tasks.

**Changes:**
- Clarified that explore is the preferred tool for read-only review tasks
- Clarified that general is preferred for implementation/fix tasks
- Noted that general remains acceptable for reviews with explicit no-edit instructions

**Flow:**
Review-only task -> use explore; implementation/fix task -> use general; optional review fallback -> use general with no-edit instructions

**Timestamp:** 2026-05-02T18:16:53.864Z

**Author:** Ian

## Narrative
### Structure
This captures a task-routing preference between two work modes rather than a code architecture detail.

### Dependencies
The choice depends on whether the work is read-only review or active implementation.

### Highlights
The user explicitly distinguished explore for verification-oriented reviews from general for implementation work.

### Rules
Going forward for this task:
- Implementation/fixes: general
- Review-only checks: explore
- If you prefer, I’ll use general for reviews too, with explicit “do not edit” instructions.

## Facts
- **review_tool_preference**: For review-only checks, explore should be used because it is read-only codebase inspection optimized for verify-this-files-against-this-spec tasks. [project]
- **implementation_tool_preference**: For implementation and fixes, general should be used. [project]
- **review_tool_fallback**: If preferred, general can also be used for reviews when given explicit do-not-edit instructions. [project]
