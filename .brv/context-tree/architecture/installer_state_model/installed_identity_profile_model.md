---
title: Installed Identity Profile Model
summary: Installed identity profile model now includes same-name concurrent installs with identity-keyed receipts, sidecars, pins, and payload storage under pkgs/by-identity.
tags: []
related: [architecture/installer_state_model/installed_identity_profile_model.overview.md, architecture/roadmap_long_horizon_items/long_horizon_spec_roadmap.md, architecture/installer_state_model/installed_identity_profile_model.abstract.md, architecture/installer_state_model/transaction_metadata_serde_io.abstract.md, architecture/installer_state_model/transaction_metadata_serde_io.overview.md]
keywords: []
createdAt: '2026-04-29T16:46:02.789Z'
updatedAt: '2026-04-29T17:08:12.142Z'
---
## Reason
Capture the change to same-name concurrent install storage and plan scope

## Raw Concept
**Task:**
Document the installed identity profile model update that expands scope from selector-only fail-closed behavior to same-name concurrent install storage.

**Changes:**
- Created an implementation plan for installed identity profile modeling.
- Adjusted scope during self-review to avoid pretending identity-scoped payload storage exists.
- Kept the first slice fail-closed for identity-scoped mutation while storage remains name-keyed.
- Proposed identity-keyed install roots derived from InstalledPackageIdentity::state_key()
- Added identity fields to receipts for profile, target, source, and package
- Moved sidecars behind the identity key to avoid cleanup collisions
- Made package payload paths identity-aware to avoid overwrites between identities
- Defined lazy compatibility for legacy installs and a future migrate-state path
- Expanded spec scope to include same-name concurrent installs
- Required identity-keyed receipts, sidecars, pins, and payload storage
- Added pkgs/by-identity/<identity-key>/<version>/ storage layout
- Added receipt identity fields for profile, target, source, and package

**Files:**
- .agents/specs/installed-identity-profile-model-spec.md
- .agents/plans/2026-04-29-installed-identity-profile-model-implementation-plan.md

**Flow:**
install selection -> identity resolution -> identity-keyed receipt/sidecar/pin write -> payload stored under by-identity path -> selected-identity uninstall removes matching storage

**Timestamp:** 2026-04-29

**Author:** Ian

**Patterns:**
- `^pkgs/by-identity/<identity-key>/<version>/$` - Identity-keyed package payload root
- `^state/installed/<identity-key>.(receipt|state.json|gui|services|integrations)$` - Identity-keyed installed state sidecar and receipt files

## Narrative
### Structure
The model now treats identity as the storage key for receipts, sidecars, pins, and package payloads instead of retaining the earlier fail-closed-only first slice.

### Dependencies
Implementation plan changes include storage-owner path work, identity receipt fields, install routing into pkgs/by-identity, and identity-scoped uninstall behavior.

### Highlights
The spec and implementation plan were updated together so concurrent installs of packages with the same name can coexist safely.

### Rules
Users should still type crosspack uninstall demo. If ambiguous, require selector arguments such as --target, --profile, and --source. Do not move old installs automatically during normal reads.

### Examples
Tests were added for concurrent same-name installs, selected-identity uninstall, sidecar isolation, and rollback storage ownership.

## Facts
- **same_name_concurrent_installs**: Same-name concurrent installs are now explicit scope for the installed identity profile model. [project]
- **identity_keyed_storage**: Identity-keyed receipts, sidecars, pins, and package payload roots are required. [project]
- **payload_layout**: The target package payload layout uses pkgs/by-identity/<identity-key>/<version>/. [project]
- **receipt_identity_fields**: Receipt identity fields include identity_profile, identity_target, identity_source, and identity_package. [project]
