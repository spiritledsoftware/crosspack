---
title: Dependency Policy v0.4 Follow-through
summary: 'Dependency-policy v0.4 follow-through completed: registry capability-provider lookup, installed-manifest stability handling, provider override error distinctions, signed fixture tests, and full workspace validation all passed.'
tags: []
related: [architecture/dependency_policy_v0_4_follow_through/provider_stability_decision.md, architecture/dependency_policy_v0_4_follow_through/dep....md, architecture/dependency_planning_boundary/installed_conflict_evidence_in_resolver_plan.overview.md, architecture/dependency_policy_v0_4_follow_through/provider_stability_decision.overview.md]
keywords: []
createdAt: '2026-05-02T00:12:02.817Z'
updatedAt: '2026-05-02T01:04:16.871Z'
---
## Reason
Capture completed implementation of dependency-policy follow-through work and validation outcomes.

## Raw Concept
**Task:**
Document the completed dependency-policy v0.4 follow-through implementation and validation.

**Changes:**
- Selected provider stability during upgrade as the first rollout slice
- Prefer an already-installed valid capability provider
- Avoid churn to a newer or different provider when the installed provider is still valid
- Implemented exact installed package/version preference for capability candidates
- Kept direct package-name precedence over provider candidates
- Added focused resolver tests
- Updated shipped docs in docs/architecture.md and docs/install-flow.md
- Confirmed the provider-stability slice is complete
- Confirmed the full dependency-policy roadmap item is still incomplete
- Captured the four remaining follow-up tasks
- Added configured-registry provider capability lookup when no direct package exists
- Plumbed installed manifests into resolver planning for exact-version provider stability
- Split provider override errors into distinct cases
- Added signed configured-registry fixture tests
- Marked the implementation plan complete

**Flow:**
plan follow-through -> implement resolver and CLI changes -> add fixture coverage -> run validation -> mark plan complete

**Timestamp:** 2026-05-01

**Author:** assistant

## Narrative
### Structure
The follow-through spans CLI metadata lookup, resolver planning, provider override validation, and signed configured-registry test coverage.

### Dependencies
Depends on resolver behavior, installed package state, configured registry metadata, and CLI planning flows.

### Highlights
Focused validation isolated the earlier timeout, and the final validation suite passed end-to-end: fmt, crosspack-resolver tests, crosspack-cli tests, clippy, workspace tests, workspace build, and git diff check.

### Rules
Provider stability is exact installed package/version preference only for capability candidates. Direct package dependencies still bypass provider stability and use the existing direct-name path.

### Examples
Provider override error distinctions included invalid shape/token, unused override, unknown provider package, non-provider package, and invalid direct-package override.

## Facts
- **reasoning_effort**: Reasoning effort was reset to medium for the follow-through work. [project]
- **configured_registry_provider_lookup**: CLI metadata resolution now searches configured registry metadata for provides capability providers when no direct package exists. [project]
- **installed_manifest_plumbing**: Upgrade/install planning now passes installed manifests into the resolver so exact-version provider stability works through CLI flows. [project]
- **provider_override_errors**: Provider override errors now distinguish invalid shape/token, unused override, unknown provider package, non-provider package, and invalid direct-package override. [project]
- **fixture_test_coverage**: Signed configured-registry fixture tests were added for provider capability resolution, installed-provider stability, conflict rejection, and replacement dry-run/explain evidence. [project]
- **plan_completion_date**: The dependency-policy follow-through plan was completed on 2026-05-01. [project]
- **validation_status**: Workspace validation passed, including fmt, resolver tests, CLI tests, clippy, workspace tests, workspace build, and git diff check. [project]
