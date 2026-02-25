# Native Installer Default (macOS/Windows) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make native installer execution the default for supported macOS/Windows installer artifacts, drop `.deb`/`.rpm` support, and harden uninstall/rollback for native installs.

**Architecture:** Introduce explicit install mode (`managed` vs `native`) and persisted native uninstall state, then thread policy and mode through install/upgrade/uninstall/rollback transaction paths. Keep Linux support focused on cross-distro/self-contained artifacts (`tar.*`, `zip`, `appimage`), and fail fast for unsupported removed distro package formats. Preserve deterministic CLI/journal/receipt behavior.

**Tech Stack:** Rust workspace (`clap`, `anyhow`, `serde`, `semver`), cargo fmt/clippy/tests, docs in `docs/`.

---

### Task 1: Remove `.deb` / `.rpm` from archive contract in core

**Files:**
- Modify: `crates/crosspack-core/src/lib.rs`
- Test: `crates/crosspack-core/src/lib.rs` (`#[cfg(test)]` module)

**Step 1: Write the failing tests**
- Add tests:
  - `archive_type_parse_rejects_deb_rpm`
  - `archive_type_infer_from_url_rejects_deb_rpm`
  - `archive_type_error_message_excludes_deb_rpm`

**Step 2: Run tests to verify failure**
- Run: `cargo test -p crosspack-core archive_type_parse_rejects_deb_rpm -- --exact`
- Expected: FAIL (current PR branch still accepts `deb`/`rpm`)

**Step 3: Implement minimal core changes**
- Remove `Deb`/`Rpm` enum variants and parse/infer branches.
- Update supported-types error text accordingly.

**Step 4: Re-run core tests**
- Run: `cargo test -p crosspack-core`
- Expected: PASS

**Step 5: Commit**
- `git commit -m "feat(core): remove deb and rpm artifact kinds"`

---

### Task 2: Remove installer handling paths for `.deb` / `.rpm`

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs` (`#[cfg(test)]` module)

**Step 1: Verify failing state after Task 1**
- Run: `cargo test -p crosspack-installer`
- Expected: FAIL/compile errors where `ArchiveType::Deb` or `ArchiveType::Rpm` are matched

**Step 2: Implement dispatch cleanup**
- Remove `deb`/`rpm` stage handlers and match arms.
- Keep supported matrix: managed (`zip`, `tar.gz`, `tar.zst`, `dmg`, `appimage`) + native (`pkg`, `exe`, `msi`, `msix`, `appx`).

**Step 3: Add/adjust support-matrix tests**
- Add test ensuring installer dispatch only contains supported kinds.

**Step 4: Re-run installer tests**
- Run: `cargo test -p crosspack-installer`
- Expected: PASS

**Step 5: Commit**
- `git commit -m "refactor(installer): drop deb rpm installer paths"`

---

### Task 3: Add install mode + native sidecar schema in installer state

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**
- Add tests:
  - `receipt_round_trip_with_install_mode_native`
  - `receipt_defaults_install_mode_managed_for_legacy`
  - `native_state_round_trip_for_uninstall_actions`

**Step 2: Run targeted failing test**
- Run: `cargo test -p crosspack-installer receipt_round_trip_with_install_mode_native -- --exact`
- Expected: FAIL

**Step 3: Implement minimal schema changes**
- Add `InstallMode` enum (`managed`, `native`) persisted on receipt.
- Add native sidecar model (uninstall action descriptors + metadata).
- Add read/write helpers and layout paths for native sidecar files.

**Step 4: Re-run installer tests**
- Run: `cargo test -p crosspack-installer`
- Expected: PASS

**Step 5: Commit**
- `git commit -m "feat(installer): persist install mode and native uninstall state"`

---

### Task 4: Add interaction/escalation policy flags and resolver in CLI

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Test: `crates/crosspack-cli/src/main.rs`

**Step 1: Write failing CLI tests**
- Add tests:
  - `install_defaults_to_auto_escalation_when_interactive`
  - `non_interactive_disables_prompt_escalation`
  - `non_interactive_allow_escalation_enables_non_prompt_paths`
  - `no_escalation_overrides_interactive_default`

**Step 2: Run targeted failing test**
- Run: `cargo test -p crosspack-cli install_defaults_to_auto_escalation_when_interactive -- --exact`
- Expected: FAIL

**Step 3: Implement policy model + flags**
- Add flags:
  - `--non-interactive`
  - `--allow-escalation`
  - `--no-escalation`
- Add policy resolver helper used by mutating commands (`install`, `upgrade`, `uninstall`, `rollback`, `repair`).

**Step 4: Re-run CLI tests**
- Run: `cargo test -p crosspack-cli`
- Expected: PASS

**Step 5: Commit**
- `git commit -m "feat(cli): add non-interactive and escalation policy flags"`

---

### Task 5: Thread install mode/policy through install and upgrade flows

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-cli/src/main.rs`, `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**
- Add tests for mode selection:
  - native default for `pkg`/`exe`/`msi`/`msix`/`appx`
  - managed mode for `zip`/`tar.*`/`dmg`/`appimage`
- Add tests that policy is propagated to installer calls.

**Step 2: Run targeted failing test**
- Run: `cargo test -p crosspack-cli native_default_mode_for_pkg -- --exact`
- Expected: FAIL

**Step 3: Implement propagation**
- Extend `install_resolved` signature and all call sites in install/upgrade paths.
- Pass mode + interaction policy into installer entrypoint.
- Update install outcome rendering to include mode when relevant.

**Step 4: Re-run crate tests**
- Run: `cargo test -p crosspack-cli`
- Run: `cargo test -p crosspack-installer`
- Expected: PASS

**Step 5: Commit**
- `git commit -m "feat(cli): propagate native install mode and policy"`

---

### Task 6: Make uninstall mode-aware and deterministic for native installs

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Modify: `crates/crosspack-cli/src/main.rs` (message/reporting only as needed)
- Test: `crates/crosspack-installer/src/lib.rs`, `crates/crosspack-cli/src/main.rs`

**Step 1: Write failing tests**
- Add tests:
  - `uninstall_native_runs_native_uninstall_before_managed_cleanup`
  - `uninstall_native_treats_not_found_as_idempotent_success`
  - `uninstall_native_failure_reports_action_context`

**Step 2: Run targeted failing test**
- Run: `cargo test -p crosspack-installer uninstall_native_runs_native_uninstall_before_managed_cleanup -- --exact`
- Expected: FAIL

**Step 3: Implement uninstall ordering**
- Read receipt mode + native sidecar.
- Execute native uninstall actions first.
- Then run existing managed cleanup (`bins`, `completions`, GUI assets/state, cache, receipt, sidecars).

**Step 4: Re-run tests**
- Run: `cargo test -p crosspack-installer`
- Expected: PASS

**Step 5: Commit**
- `git commit -m "feat(installer): add mode-aware native uninstall flow"`

---

### Task 7: Expand rollback snapshot/journal replay for native side effects

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-installer/src/lib.rs` (helpers/state access if required)
- Test: `crates/crosspack-cli/src/main.rs`

**Step 1: Write failing rollback tests**
- Add tests:
  - `capture_snapshot_includes_completions_gui_and_native_state`
  - `rollback_replays_native_uninstall_before_managed_restore`
  - `repair_handles_interrupted_native_transaction`

**Step 2: Run targeted failing test**
- Run: `cargo test -p crosspack-cli rollback_replays_native_uninstall_before_managed_restore -- --exact`
- Expected: FAIL

**Step 3: Implement rollback enhancements**
- Expand snapshot capture/restore coverage beyond package dir/receipt/bins.
- Add native step names in transaction journal.
- Update replay mapping/order to reverse native side effects first, then restore managed snapshot state.

**Step 4: Re-run CLI tests**
- Run: `cargo test -p crosspack-cli`
- Expected: PASS

**Step 5: Commit**
- `git commit -m "feat(cli): harden rollback for native installer side effects"`

---

### Task 8: Update docs and run full CI-equivalent verification

**Files:**
- Modify: `docs/install-flow.md`
- Modify: `docs/architecture.md`
- Modify: `docs/manifest-spec.md`
- Modify: `docs/transaction-rollback-spec.md`

**Step 1: Write docs-first failing check**
- Run: `cargo test --workspace`
- Expected: May pass/fail; baseline before doc+final polish

**Step 2: Update docs**
- Document:
  - native-default matrix for macOS/Windows installer kinds
  - removed `.deb`/`.rpm` support
  - non-interactive/escalation policy behavior
  - native-aware rollback model

**Step 3: Run formatting and quality gates**
- Run: `cargo fmt --all --check`
- Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Run: `cargo test --workspace`
- Expected: PASS

**Step 4: Final commit**
- `git commit -m "docs(install): align native-default behavior and rollback contract"`

**Step 5: Optional PR polish**
- Capture concise changelog bullets for PR description update.

---
