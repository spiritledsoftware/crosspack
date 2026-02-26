# macOS Launcher Bundle Registration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make macOS GUI app registration launchpad-friendly by deploying real `.app` bundles (not symlinks), preferring `/Applications`, with safe fallback and ownership protection.

**Architecture:** Keep Crosspack's existing managed install root behavior, but change native macOS GUI registration to deploy a concrete app bundle copy for `.app` sources. Registration attempts `/Applications` first and falls back to `~/Applications` if blocked, while refusing to overwrite unmanaged existing apps. Native state records track deployment kind so uninstall/upgrade cleanup remains deterministic and safe.

**Tech Stack:** Rust (`crosspack-cli`, `crosspack-installer`), existing native GUI sidecar state, macOS LaunchServices (`lsregister`), cargo test/clippy.

---

### Task 1: Define installer API change for ownership-aware macOS registration

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Modify: `crates/crosspack-cli/src/main.rs`
- Test: `crates/crosspack-cli/src/main.rs` (existing tests for registration sync helpers)

**Step 1: Write failing test (CLI call-site contract)**

Add/adjust a CLI-level unit test that validates sync logic can pass prior native records to installer registration (or fails to compile before signature update).

**Step 2: Run test to verify it fails**

Run: `rustup run stable cargo test -p crosspack-cli`
Expected: FAIL/compile error referencing outdated `register_native_gui_app_best_effort(...)` call signature.

**Step 3: Write minimal implementation**

Update installer API signature to accept previous native records context and wire CLI `sync_native_gui_registration_state_best_effort(...)` to pass `previous_records`.

**Step 4: Run test to verify it passes**

Run: `rustup run stable cargo test -p crosspack-cli`
Expected: PASS for updated call path and existing CLI behavior.

**Step 5: Commit**

```bash
git add crates/crosspack-installer/src/lib.rs crates/crosspack-cli/src/main.rs
git commit -m "refactor(gui): pass prior native records into macOS registration"
```

### Task 2: Add macOS destination selection and unmanaged-overwrite protection

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests for helper behavior:
- prefers `/Applications` when writable and safe,
- falls back to `~/Applications` if system destination is unavailable,
- refuses to overwrite unmanaged existing app bundle at target path,
- allows replacement when previous native records prove Crosspack ownership.

**Step 2: Run targeted tests to verify failure**

Run: `rustup run stable cargo test -p crosspack-installer macos_registration`
Expected: FAIL because destination-selection/ownership-guard helpers are not implemented.

**Step 3: Write minimal implementation**

In installer macOS code:
- resolve `.app` bundle root from `macos_registration_source_path(...)`,
- compute candidate destinations (`/Applications/<App>.app`, `~/Applications/<App>.app`),
- check existing target ownership against prior records,
- skip unmanaged existing targets with warning, continue fallback.

**Step 4: Run targeted tests to verify pass**

Run: `rustup run stable cargo test -p crosspack-installer macos_registration`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "feat(macos): select Applications target with unmanaged overwrite guard"
```

### Task 3: Replace macOS `.app` symlink registration with bundle-copy deployment

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests asserting:
- `.app` source registration produces deployed app directory (copy) rather than symlink,
- `lsregister -f` is invoked against deployed bundle path,
- non-`.app` GUI registrations keep existing symlink behavior.

**Step 2: Run targeted tests to verify failure**

Run: `rustup run stable cargo test -p crosspack-installer register_native_gui`
Expected: FAIL due to existing symlink-only behavior.

**Step 3: Write minimal implementation**

Implement macOS registration branch changes:
- for `.app` bundle sources, copy bundle recursively to selected destination,
- preserve best-effort warning behavior,
- keep non-bundle paths on legacy symlink path.

**Step 4: Run targeted tests to verify pass**

Run: `rustup run stable cargo test -p crosspack-installer register_native_gui`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "fix(macos): register GUI apps as real bundle copies for launcher indexing"
```

### Task 4: Introduce native record kind for bundle copies and cleanup semantics

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests asserting:
- native state persists new kind (e.g. `applications-bundle-copy`),
- uninstall action removes bundle copies recursively,
- existing `applications-symlink` behavior remains unchanged,
- stale native cleanup handles both kinds.

**Step 2: Run targeted tests to verify failure**

Run: `rustup run stable cargo test -p crosspack-installer uninstall_native`
Expected: FAIL because new kind is unsupported and cleanup semantics are incomplete.

**Step 3: Write minimal implementation**

Update native record generation and removal paths:
- emit `applications-bundle-copy` for copied macOS bundles,
- support recursive removal for this kind,
- keep symlink kind logic and idempotency.

**Step 4: Run targeted tests to verify pass**

Run: `rustup run stable cargo test -p crosspack-installer uninstall_native`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "feat(macos): track bundle-copy native records for deterministic cleanup"
```

### Task 5: Keep DMG payload hygiene behavior explicit

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing test**

Retain/repair `copy_dmg_payload_skips_root_applications_symlink` coverage so DMG root `Applications -> /Applications` is never copied into package install root.

**Step 2: Run test to verify it fails (if currently red)**

Run: `rustup run stable cargo test -p crosspack-installer copy_dmg_payload_skips_root_applications_symlink`
Expected: FAIL until helper implementation is present.

**Step 3: Write minimal implementation**

Implement/restore `copy_dmg_payload(...)` and hook `stage_dmg_payload_with_hooks(...)` to use it, skipping only root-level `Applications` symlink.

**Step 4: Run test to verify pass**

Run: `rustup run stable cargo test -p crosspack-installer copy_dmg_payload_skips_root_applications_symlink`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "fix(dmg): skip root Applications symlink when staging payload"
```

### Task 6: Update policy documentation

**Files:**
- Modify: `docs/manifest-spec.md`
- Modify: `docs/install-flow.md`

**Step 1: Write failing doc assertion check (manual checklist)**

Create a checklist ensuring docs mention:
- macOS GUI registration now prefers `/Applications` bundle copy,
- fallback to `~/Applications`,
- unmanaged overwrite protection,
- warning-driven best-effort behavior retained.

**Step 2: Verify current docs fail checklist**

Run: manual review of `docs/manifest-spec.md` and `docs/install-flow.md`.
Expected: Missing/newly inaccurate behavior statements before edits.

**Step 3: Write minimal doc updates**

Update Native GUI Registration policy and install flow sections with new macOS behavior and ownership guardrails.

**Step 4: Verify checklist passes**

Run: manual review of updated docs.
Expected: Checklist complete and aligned with implementation.

**Step 5: Commit**

```bash
git add docs/manifest-spec.md docs/install-flow.md
git commit -m "docs(macos): document bundle-copy GUI registration and fallback policy"
```

### Task 7: Full verification

**Files:**
- Modify: none
- Test: workspace verification commands

**Step 1: Run installer tests**

Run: `rustup run stable cargo test -p crosspack-installer`
Expected: PASS.

**Step 2: Run installer lint checks**

Run: `rustup run stable cargo clippy -p crosspack-installer --all-targets -- -D warnings`
Expected: PASS.

**Step 3: Run CLI tests**

Run: `rustup run stable cargo test -p crosspack-cli`
Expected: PASS.

**Step 4: Commit verification note (if needed)**

If any follow-up fix was required for verification, commit that minimal fix separately.

### Task 8: PR updates and rollout verification

**Files:**
- Modify: existing PR branch in `crosspack`

**Step 1: Update PR with final commits**

Push commits to existing branch/PR (`crosspack` PR #64) unless reviewer requests split PR.

**Step 2: Update PR description**

Document policy:
- system-first bundle copy,
- user-scope fallback,
- no unmanaged overwrite,
- cleanup record kind changes.

**Step 3: Tahoe manual validation steps**

Run on macOS Tahoe:
- `crosspack uninstall neovide`
- `crosspack install neovide --force-redownload`
- verify app appears in default launcher.

**Step 4: Record observed behavior**

Capture whether registration landed in `/Applications` or fallback `~/Applications`, and confirm visibility without manual reindex steps.
