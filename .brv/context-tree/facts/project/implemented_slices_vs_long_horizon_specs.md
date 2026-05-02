---
consolidated_at: '2026-04-29T17:24:11.673Z'
consolidated_from:
  - {date: '2026-04-29T17:24:11.673Z', path: facts/project/roadmap_long_horizon_items.md, reason: 'These files cover the same long-horizon spec set and overlapping status/roadmap framing. The implemented-slices note is the richer, more specific version, while the roadmap note is a shorter summary of the same seven areas, so they should be consolidated into one durable project roadmap/status record.'}
---
# Title: Implemented Slices vs Long-Horizon Specs

## Summary
Several long-horizon specs already have partial implementations; they are not all greenfield, but each still has explicit remaining work.

## Reason
Capture the clarified implementation status of the major follow-through specs and the durable roadmap items they represent.

## Raw Concept
**Task:**
Clarify implementation status of long-horizon follow-through specs and capture the remaining roadmap areas.

**Changes:**
- Confirmed that the listed specs are not greenfield
- Identified which areas are already partially implemented
- Separated shipped slices from remaining work
- Identified seven durable long-horizon roadmap areas spanning product, policy, reliability, integrations, documentation, and registry automation

**Flow:**
user asks if features exist -> assistant clarifies partial implementation -> remaining scope is documented -> roadmap items are preserved

**Author:** Ian

## Narrative
### Structure
The response organizes major spec areas into implemented, partially implemented, and not complete status, and also records the durable roadmap items that remain.

### Dependencies
The clarification depends on shipped slices already present in code and documentation, but each area still needs follow-through work.

### Highlights
The key conclusion is that these specs are long-horizon follow-through specs rather than pure greenfield specifications. The roadmap also preserves seven durable areas still needing work.

### Examples
Examples include installed identity/profile model, dependency policy v0.4, transaction recovery v0.5, typed host integrations, no post-install scripts policy, registry automation maturation, and docs/spec drift control.

## Facts
- **installed_identity_profile_model_status**: Installed identity/profile model is partially implemented, including identity-keyed installed-state docs, legacy hydration, and ambiguity checks in lifecycle paths. [project]
- **dependency_policy_v0_4_status**: Dependency policy v0.4 is partially implemented, including provides, conflicts, replaces, typed InstallPlan evidence, dry-run rendering, and replacement handoff. [project]
- **transaction_recovery_v0_5_status**: Transaction recovery v0.5 is partially implemented, including typed transaction status, coordinator routing, rollback/repair/doctor behavior, and snapshot replay. [project]
- **typed_host_integrations_status**: Typed host integrations are partially implemented, including [[integrations]], Docker/PATH/service metadata projection, sidecar state, and uninstall cleanup. [project]
- **no_post_install_scripts_policy_status**: No post-install scripts policy mostly aligns with product direction, but still lacks an explicitly codified policy, validation/lint coverage for script-like fields, and docs enforcement. [project]
- **registry_automation_maturation_status**: Registry automation maturation is partially implemented, including source strategies, signing workflows, proportional quality-gate work, retry/cache/rate-limit improvements. [project]
- **docs_spec_drift_control_status**: Docs/spec drift control is partially implemented, with shipped behavior vs roadmap distinctions and some release checklist guardrails, but no systematic drift checks or PR checklist enforcement yet. [project]
- **follow_through_specs_intent**: These specs are intended to capture remaining architecture and product horizon work while explicitly acknowledging shipped slices. [project]
- **roadmap_long_horizon_items**: The roadmap captures seven durable long-horizon items: installed identity/profile v2, dependency policy v0.4, transaction/recovery v0.5 hardening, typed host integrations, no post-install scripts, registry automation, and docs/spec drift control. [project]
- **roadmap_alignment**: These items require continued alignment between shipped behavior and documentation. [project]
- **reasoning_effort**: Reasoning effort was set to medium. [project]