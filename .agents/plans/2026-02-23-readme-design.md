# Crosspack README Design

## Goal

Create a comprehensive and attractive `README.md` that serves both prospective users and contributors without duplicating detailed specs already maintained under `docs/`.

## Audience

- Primary: both users and contributors equally.
- Secondary: maintainers needing a stable top-level project narrative.

## Design Principles

- Lead with value and trust model in plain language.
- Keep quick-start actionable with copy-paste commands.
- Preserve technical credibility by matching actual CLI and documented behavior.
- Keep details layered: concise top-level content, links to deeper docs for policy and roadmap.

## Proposed Structure

1. Project identity and one-sentence value proposition.
2. Why Crosspack and project goals.
3. Current capabilities.
4. Prerequisites.
5. Quick start (build, source setup, search/info/install, pin/upgrade/uninstall, init-shell).
6. Legacy `--registry-root` mode note.
7. Command reference table.
8. Security model and explicit trust-boundary caveat.
9. Install layout and default prefixes.
10. Workspace architecture crate map.
11. Development quality gates and snapshot validation script.
12. Documentation map to authoritative specs.
13. Roadmap note.
14. Contribution checklist and license.

## Accuracy Constraints

- Use command forms verified from current CLI help output.
- Keep terminology aligned with `docs/architecture.md`, `docs/install-flow.md`, and `docs/registry-spec.md`.
- Avoid claiming unimplemented behavior as complete features; describe v0.4 and v0.5 as roadmap targets.

## Copy and Tone

- Friendly but technical.
- Short paragraphs and high-signal bullet lists.
- Minimal marketing language; focus on clarity and trust.

## Success Criteria

- New reader can understand what Crosspack is and why it exists in under one minute.
- New user can run a first end-to-end local flow from README commands.
- New contributor can find architecture, quality gates, and deeper specs quickly.
- Content remains consistent with current codebase and docs.
