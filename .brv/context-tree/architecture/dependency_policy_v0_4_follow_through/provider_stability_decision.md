---
title: Provider Stability Decision
summary: Provider stability should prefer the exact installed provider version already present to avoid upgrade churn.
tags: []
related: [architecture/dependency_policy_v0_4_follow_through/dependency_policy_v0_4_follow_through.md]
keywords: []
createdAt: '2026-05-02T00:17:54.234Z'
updatedAt: '2026-05-02T00:17:54.234Z'
---
## Reason
Capture the clarified decision on provider stability for the plan slice

## Raw Concept
**Task:**
Document provider stability choice for dependency policy follow-through

**Changes:**
- Chose exact installed provider version over broader provider package upgrade behavior

**Flow:**
Select exact installed provider version -> avoid upgrade churn -> match current plan wording

**Timestamp:** 2026-05-02T00:17:31.878Z

## Narrative
### Structure
The plan slice uses exact installed provider version stability rather than package-name-level upgrade preference.

### Dependencies
This choice is aligned with the current plan wording.

### Highlights
Preferred answer is to keep the exact installed provider version; it is the smallest change and avoids upgrade churn.
