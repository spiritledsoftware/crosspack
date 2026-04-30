# No Post-Install Scripts Policy Spec

**Status:** roadmap, non-GA
**Last updated:** 2026-04-29

## Problem

Arbitrary post-install scripts are powerful but undermine Crosspack's goals: deterministic installs, previewable plans, safe rollback, registry trust, and clear host integration boundaries. Crosspack needs a written product policy that rejects arbitrary lifecycle scripts and channels common needs into typed, reviewable capabilities.

## Goals

- Codify that packages cannot run arbitrary post-install scripts.
- Define approved typed alternatives for common install-time needs.
- Make registry review and automation enforce the policy.
- Preserve rollback and uninstall safety.
- Keep package behavior understandable from manifest metadata.

## Non-Goals

- Do not support maintainer-provided shell hooks as a compatibility feature.
- Do not add a sandboxed script runner as an escape hatch.
- Do not silently execute host package managers, service managers, or shell profile edits.
- Do not make registry maintainers manually inspect opaque generated scripts.

## Current State

- Crosspack install flow is declarative and transaction-oriented.
- Typed integrations are emerging as a safer replacement for script-like behavior.
- Native installers can be staged/executed by artifact type policy, but arbitrary package scripts are not a supported package mechanism.

## Target Behavior

Registry package metadata must express behavior through typed fields:

- binaries
- completions
- GUI apps
- typed integrations
- declared services
- source build commands where explicitly opted into and deterministic
- provider/conflict/replacement policy

Rejected metadata:

```toml
post_install = "curl https://example.test/install.sh | sh"

[scripts]
post_install = "mkdir -p ~/.docker/cli-plugins && cp ..."
```

Approved equivalent:

```toml
[[integrations]]
kind = "docker_cli_plugin"
name = "compose"
source = "docker-compose"
```

## Architecture

The manifest schema and registry validation should make unsupported script fields impossible to accept silently.

```text
registry package metadata
        |
        v
schema validation ----> reject script-like fields
        |
        v
typed capabilities ----> installer-owned adapters
```

Typed capabilities are implemented by Crosspack code, not package-maintainer scripts.

## Data/State Model

No script field should be added to package manifests.

If a future exceptional capability is required, it must be represented as:

- a typed manifest field,
- a typed installer plan action,
- persisted state for rollback/uninstall,
- deterministic validation rules,
- tests for failure and cleanup behavior.

## CLI/UX Contracts

When metadata contains unsupported lifecycle script fields, fail with explicit guidance:

```text
manifest validation failed: arbitrary lifecycle scripts are not supported; use typed integrations or declared services
```

If users need host activation, expose explicit commands rather than hidden install-time hooks:

```text
crosspack integrations enable <package> <integration>
crosspack services enable <package> <service>
```

## Failure Modes

- Unknown script-like field in registry metadata: reject during validation.
- Generated registry package includes script-like key: fail quality gate.
- Source build command attempts to act like post-install host mutation: fail source build policy review or validation.
- Typed capability missing for a needed behavior: package remains metadata-only until capability exists.

## Testing Requirements

- Manifest unknown-field tests reject script-like fields.
- Registry validation rejects `post_install`, `pre_install`, generic `[scripts]`, and lifecycle aliases.
- Docs tests or lint checks prevent shipped docs from recommending script hooks.
- Source build tests keep build/install commands scoped to staged build outputs, not host mutation.
- Integration capability tests cover the approved replacements.

## Rollout Plan

1. Write the policy in shipped docs once wording is final.
2. Add manifest/registry validation tests for script-like fields.
3. Audit registry metadata and generation scripts for script escape hatches.
4. Add typed capabilities for the highest-value script replacement needs.
5. Keep policy checks in CI for registry PRs.

## Open Questions

- Should source-build `install` commands be further constrained to staged prefixes only?
- Should native installer artifacts be documented as distinct from package lifecycle scripts?
- Should Crosspack expose a policy reason code for rejected script fields?
