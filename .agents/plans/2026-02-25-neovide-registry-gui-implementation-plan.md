# Neovide Registry GUI Entry Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `neovide@0.15.2` to the registry with deterministic GUI metadata for Linux, macOS, and Windows so GUI lifecycle can be validated end-to-end.

**Architecture:** This is a registry metadata change in `crosspack-registry` plus runtime verification in `crosspack`. Create one new manifest with three host-target artifacts and one `gui_apps` declaration per artifact. Keep GUI declarations minimal (no protocols or file associations) so tests exercise deterministic GUI exposure/state behavior without additional host-registration complexity.

**Tech Stack:** TOML manifests, Python validation scripts, Cargo (`crosspack-cli`), local registry-root installs.

---

### Task 1: Create the Neovide Manifest

**Files:**
- Create: `crosspack-registry/index/neovide/0.15.2.toml`
- Test: `crosspack-registry/scripts/registry-validate-entry.py`

**Step 1: Write the failing test**

Run (from `crosspack-registry`): `python3 scripts/registry-validate-entry.py index/neovide/0.15.2.toml`
Expected: FAIL with file-not-found because manifest does not exist yet.

**Step 2: Write minimal implementation**

Create `crosspack-registry/index/neovide/0.15.2.toml` with this exact content:

```toml
name = "neovide"
version = "0.15.2"
license = "MIT"
homepage = "https://github.com/neovide/neovide"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://github.com/neovide/neovide/releases/download/0.15.2/neovide-linux-x86_64.tar.gz"
sha256 = "0965d5a0fecfd00b0ba12a14e1eb38e2f47efee7cf5d36f5aed36b008aa87138"
archive = "tar.gz"
strip_components = 0

[[artifacts.binaries]]
name = "neovide"
path = "neovide"

[[artifacts.gui_apps]]
app_id = "io.neovide.neovide"
display_name = "Neovide"
exec = "neovide"
categories = ["Utility", "Development"]

[[artifacts]]
target = "x86_64-apple-darwin"
url = "https://github.com/neovide/neovide/releases/download/0.15.2/Neovide-x86_64-apple-darwin.dmg"
sha256 = "c7a8d29cc75d72a943c91d2519429b5dc5c55f9a50ea28bb80bb2272f60ddb13"
strip_components = 1

[[artifacts.binaries]]
name = "neovide"
path = "Neovide.app/Contents/MacOS/neovide"

[[artifacts.gui_apps]]
app_id = "io.neovide.neovide"
display_name = "Neovide"
exec = "Neovide.app/Contents/MacOS/neovide"
categories = ["Utility", "Development"]

[[artifacts]]
target = "x86_64-pc-windows-msvc"
url = "https://github.com/neovide/neovide/releases/download/0.15.2/neovide.exe.zip"
sha256 = "8ad43abbb2d012aa9d89585349f589723cf01dc65607d417059923c623afd922"
archive = "zip"
strip_components = 0

[[artifacts.binaries]]
name = "neovide"
path = "neovide.exe"

[[artifacts.gui_apps]]
app_id = "io.neovide.neovide"
display_name = "Neovide"
exec = "neovide.exe"
categories = ["Utility", "Development"]
```

**Step 3: Run test to verify it passes**

Run (from `crosspack-registry`): `python3 scripts/registry-validate-entry.py index/neovide/0.15.2.toml`
Expected: PASS.

**Step 4: Run smoke-install validation**

Run (from `crosspack-registry`): `python3 scripts/registry-smoke-install.py index/neovide/0.15.2.toml`
Expected: PASS on Linux host target with extracted `neovide` binary present.

**Step 5: Commit**

```bash
git add index/neovide/0.15.2.toml
git commit -m "chore(registry): add neovide@0.15.2 GUI manifest"
```

### Task 2: Keep Signature Work Out of Scope (per requirement)

**Files:**
- No file changes required
- Reference: `crosspack-registry/.github/workflows/sign-manifests-on-merge.yml`

**Step 1: Verify current policy**

Run (from `crosspack-registry`): `python3 scripts/registry-validate-entry.py index/neovide/0.15.2.toml`
Expected: PASS (entry-level schema check only, no sidecar requirement).

**Step 2: Confirm no local signature generation**

Do not create or edit `index/neovide/0.15.2.toml.sig` in this task.

**Step 3: Verify working tree content**

Run (from `crosspack-registry`): `git status --short`
Expected: only intended manifest file change (plus any unrelated pre-existing changes).

**Step 4: Capture PR note**

Add to PR description: "Signature sidecars are handled by merge automation (`sign-manifests-on-merge.yml`)."

**Step 5: Commit**

No additional commit in this task.

### Task 3: Validate Crosspack End-to-End Install/Uninstall with GUI Assets

**Files:**
- Test runtime: `crosspack/crates/crosspack-cli/src/main.rs` (no edits expected)
- Generated state (temporary): `$TMP_HOME/.crosspack/**`

**Step 1: Write failing precondition check**

Run (from `crosspack`):

```bash
TMP_HOME="$PWD/.tmp/neovide-e2e-home"
rm -rf "$TMP_HOME"
mkdir -p "$TMP_HOME"
test ! -e "$TMP_HOME/.crosspack/state/installed/neovide.gui"
```

Expected: PASS (GUI state absent before install).

**Step 2: Run deterministic dry-run**

Run (from `crosspack`):

```bash
HOME="$TMP_HOME" cargo run -p crosspack-cli -- --registry-root ../crosspack-registry install neovide --dry-run
```

Expected: deterministic transaction preview lines (`transaction_*`, `risk_flags`, `change_*`).

**Step 3: Run install apply path**

Run (from `crosspack`):

```bash
HOME="$TMP_HOME" cargo run -p crosspack-cli -- --registry-root ../crosspack-registry install neovide
```

Expected: successful install with GUI asset exposure.

**Step 4: Verify GUI outputs**

Run (from `crosspack`):

```bash
test -f "$TMP_HOME/.crosspack/state/installed/neovide.gui"
ls "$TMP_HOME/.crosspack/share/gui/launchers"
ls "$TMP_HOME/.crosspack/share/gui/handlers"
```

Expected: state sidecar exists and launcher/handler assets are present.

**Step 5: Verify uninstall cleanup**

Run (from `crosspack`):

```bash
HOME="$TMP_HOME" cargo run -p crosspack-cli -- uninstall neovide
test ! -e "$TMP_HOME/.crosspack/state/installed/neovide.gui"
```

Expected: uninstall succeeds and GUI state sidecar is removed.

### Task 4: Final Validation and PR Readiness

**Files:**
- Optional modify: `crosspack-registry/README.md` (only if maintainers want Platform Coverage text updated)

**Step 1: Run final registry checks**

Run (from `crosspack-registry`):

```bash
python3 scripts/registry-validate-entry.py index/neovide/0.15.2.toml
python3 scripts/registry-smoke-install.py index/neovide/0.15.2.toml
```

Expected: both PASS.

**Step 2: Check final diff**

Run (from `crosspack-registry`): `git status --short`
Expected: contains `index/neovide/0.15.2.toml` and only intended files.

**Step 3: Optional coverage docs update**

If requested by maintainers, add a Neovide line in `README.md` Platform Coverage.

**Step 4: Optional docs commit**

```bash
git add README.md
git commit -m "docs(registry): update platform coverage for neovide"
```

**Step 5: Prepare PR summary**

Include:
- Added `neovide@0.15.2` manifest with Linux/macOS/Windows artifacts.
- Added deterministic `gui_apps` metadata per artifact.
- Verified local schema + smoke-install + crosspack install/uninstall GUI lifecycle checks.
