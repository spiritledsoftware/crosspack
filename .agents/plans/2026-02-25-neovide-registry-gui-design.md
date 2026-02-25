# Design: Add GUI Package to Registry for Deterministic End-to-End Validation

Date: 2026-02-25
Owner: OpenCode (with user approval)

## Goal

Add one real GUI application package to `crosspack-registry` so deterministic GUI install flows can be validated end-to-end across Linux, macOS, and Windows.

## Approved Direction

Use `neovide@0.15.2` as the first GUI package in the registry.

Why this package:

- Small, practical artifacts (~12 MB each) keep smoke and e2e loops fast.
- Publishes platform-native assets for all requested targets.
- Artifact structure is compatible with current deterministic extraction paths.

## Scope

In scope:

- Add a new manifest at `index/neovide/0.15.2.toml` in `crosspack-registry`.
- Include three targets:
  - `x86_64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `x86_64-pc-windows-msvc`
- Declare one binary and one GUI app per artifact.
- Keep GUI metadata minimal and deterministic.

Out of scope:

- Adding aarch64 targets in this change.
- Adding protocol handlers or file associations in this first GUI registry entry.
- Any changes to crosspack CLI/installer/core crates.

## Manifest Design

Package metadata:

- `name = "neovide"`
- `version = "0.15.2"`
- `license` and `homepage` set to upstream values.

Artifacts:

1) Linux (`x86_64-unknown-linux-gnu`)

- URL: `https://github.com/neovide/neovide/releases/download/0.15.2/neovide-linux-x86_64.tar.gz`
- `archive = "tar.gz"`
- `strip_components = 0`
- Binary:
  - `name = "neovide"`
  - `path = "neovide"`
- GUI app:
  - `app_id = "io.neovide.neovide"`
  - `display_name = "Neovide"`
  - `exec = "neovide"`

2) macOS (`x86_64-apple-darwin`)

- URL: `https://github.com/neovide/neovide/releases/download/0.15.2/Neovide-x86_64-apple-darwin.dmg`
- DMG type inferred from URL (no explicit `archive` required)
- `strip_components = 1` to normalize mounted DMG root layout into staged payload
- Binary:
  - `name = "neovide"`
  - `path = "Neovide.app/Contents/MacOS/neovide"`
- GUI app:
  - `app_id = "io.neovide.neovide"`
  - `display_name = "Neovide"`
  - `exec = "Neovide.app/Contents/MacOS/neovide"`

3) Windows (`x86_64-pc-windows-msvc`)

- URL: `https://github.com/neovide/neovide/releases/download/0.15.2/neovide.exe.zip`
- `archive = "zip"`
- `strip_components = 0`
- Binary:
  - `name = "neovide"`
  - `path = "neovide.exe"`
- GUI app:
  - `app_id = "io.neovide.neovide"`
  - `display_name = "Neovide"`
  - `exec = "neovide.exe"`

Shared GUI policy in this first entry:

- No `protocols` declarations yet.
- No `file_associations` declarations yet.
- Optional `categories` may be included, but no host-level handler claims.

## Signature Handling

- Do not perform local signing work in this task.
- Rely on PR merge automation in `crosspack-registry` to write/update sidecar signatures.

## Data Flow and Runtime Expectations

When users install `neovide`:

1. Crosspack resolves host-target artifact from manifest.
2. Artifact is downloaded and SHA-256 verified.
3. Deterministic staging/extraction runs by artifact type:
   - Linux tar.gz extract
   - macOS DMG attach/copy/detach flow
   - Windows zip extract
4. Binary is exposed via prefix `bin`.
5. GUI launcher/handler assets are generated under prefix `share/gui`.
6. GUI sidecar state is persisted under `state/installed/neovide.gui` and best-effort native registration state is tracked in `.gui-native`.

## Validation Plan

Registry repo validation:

- `python3 scripts/registry-validate.py index/neovide/0.15.2.toml`
- `python3 scripts/registry-smoke-install.py index/neovide/0.15.2.toml`
- `./scripts/registry-preflight.sh`

Crosspack e2e validation (Linux host):

- `crosspack update`
- `crosspack install neovide --dry-run`
- `crosspack install neovide`
- Verify expected GUI artifacts and sidecars in prefix.
- `crosspack uninstall neovide` and verify cleanup.

## Acceptance Criteria

- Manifest is schema-valid and CI preflight-clean.
- Linux smoke-install check passes for the new entry.
- Crosspack install flow produces GUI assets/state for `neovide`.
- Uninstall removes exposed GUI assets/state cleanly.

## Risks and Mitigations

- DMG payload layout mismatch risk:
  - Mitigation: keep macOS path explicitly aligned with known release layout and use `strip_components = 1`.
- Upstream release asset drift risk:
  - Mitigation: pin exact versioned URLs and SHA-256 digests.
- Host registration side effects risk:
  - Mitigation: start without protocol/file association claims.
