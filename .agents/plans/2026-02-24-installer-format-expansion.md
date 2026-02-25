# Installer Format Expansion (EXE/PKG/DEB/RPM/MSIX/APPX) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add deterministic, fail-closed support for `exe`, `pkg`, `deb`, `rpm`, `msix`, and `appx` artifact ingestion so GUI packages can install across all major platforms without vendor-installer execution fallback.

**Architecture:** Extend `ArchiveType` and installer staging dispatch so each artifact kind has a dedicated adapter with explicit host constraints and actionable errors. Keep extraction deterministic (archive/container unpack only) and never execute installer UI flows or maintainer scripts. Preserve existing install/upgrade/uninstall contracts and best-effort native GUI registration semantics.

**Tech Stack:** Rust workspace (`crosspack-core`, `crosspack-installer`, `crosspack-cli`), `std::process::Command`, existing staging/copy pipeline in `crosspack-installer`, docs in `docs/`.

---

### Task 0: Baseline verification before changes

**Files:**
- Modify: none

**Step 1: Verify branch state**

Run:
```bash
git status -sb
```

Expected: branch is correct for feature work; unrelated local changes are understood and preserved.

**Step 2: Verify baseline tests**

Run:
```bash
cargo test --workspace
```

Expected: PASS.

---

### Task 1: Extend artifact kind schema in core

**Files:**
- Modify: `crates/crosspack-core/src/lib.rs`
- Test: `crates/crosspack-core/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `archive_type_parse_supports_exe_pkg_deb_rpm_msix_appx`
- `archive_type_infer_from_url_supports_exe_pkg_deb_rpm_msix_appx`
- `manifest_allows_gui_package_with_exe_installer_artifact_kind`

Test snippet:
```rust
#[test]
fn archive_type_parse_supports_exe_pkg_deb_rpm_msix_appx() {
    assert_eq!(ArchiveType::parse("exe"), Some(ArchiveType::Exe));
    assert_eq!(ArchiveType::parse("pkg"), Some(ArchiveType::Pkg));
    assert_eq!(ArchiveType::parse("deb"), Some(ArchiveType::Deb));
    assert_eq!(ArchiveType::parse("rpm"), Some(ArchiveType::Rpm));
    assert_eq!(ArchiveType::parse("msix"), Some(ArchiveType::Msix));
    assert_eq!(ArchiveType::parse("appx"), Some(ArchiveType::Appx));
}
```

**Step 2: Run targeted test to verify failure**

Run:
```bash
cargo test -p crosspack-core archive_type_parse_supports_exe_pkg_deb_rpm_msix_appx -- --exact
```

Expected: FAIL before enum/parser updates.

**Step 3: Implement minimal schema support**

In `crates/crosspack-core/src/lib.rs`:
- Extend `ArchiveType` with `Exe`, `Pkg`, `Deb`, `Rpm`, `Msix`, `Appx`.
- Update:
  - `as_str()`
  - `cache_extension()`
  - `parse()`
  - `infer_from_url()`
- Update unsupported archive message to include all supported kinds.

Implementation snippet:
```rust
if lower.ends_with(".exe") {
    return Some(Self::Exe);
}
if lower.ends_with(".pkg") {
    return Some(Self::Pkg);
}
if lower.ends_with(".deb") {
    return Some(Self::Deb);
}
```

**Step 4: Run crate tests**

Run:
```bash
cargo test -p crosspack-core
```

Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-core/src/lib.rs
git commit -m "feat(core): add exe pkg deb rpm msix appx artifact kinds"
```

---

### Task 2: Add staging dispatcher routes and host guards

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `install_from_artifact_rejects_exe_on_non_windows_host`
- `install_from_artifact_rejects_pkg_on_non_macos_host`
- `install_from_artifact_rejects_deb_on_non_linux_host`
- `install_from_artifact_rejects_rpm_on_non_linux_host`
- `install_from_artifact_rejects_msix_on_non_windows_host`
- `install_from_artifact_rejects_appx_on_non_windows_host`

**Step 2: Run targeted tests to verify failure**

Run:
```bash
cargo test -p crosspack-installer install_from_artifact_rejects_exe_on_non_windows_host -- --exact
cargo test -p crosspack-installer install_from_artifact_rejects_pkg_on_non_macos_host -- --exact
```

Expected: FAIL before dispatcher updates.

**Step 3: Implement minimal dispatcher and guard errors**

In `crates/crosspack-installer/src/lib.rs`:
- Extend `stage_artifact_payload(...)` match arms:
  - `ArchiveType::Exe => stage_exe_payload(...)`
  - `ArchiveType::Pkg => stage_pkg_payload(...)`
  - `ArchiveType::Deb => stage_deb_payload(...)`
  - `ArchiveType::Rpm => stage_rpm_payload(...)`
  - `ArchiveType::Msix => stage_msix_payload(...)`
  - `ArchiveType::Appx => stage_appx_payload(...)`
- Add host-guard stubs that return actionable errors.

Stub snippet:
```rust
fn stage_pkg_payload(_artifact_path: &Path, _raw_dir: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(anyhow!("PKG artifacts are supported only on macOS hosts"));
    }
    Err(anyhow!("PKG staging is not implemented yet"))
}
```

**Step 4: Run crate tests**

Run:
```bash
cargo test -p crosspack-installer
```

Expected: PASS with stub behavior and new guard tests.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "refactor(installer): route new installer artifact kinds with host guards"
```

---

### Task 3: Implement EXE deterministic extraction adapter (Windows)

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `stage_exe_builds_extract_command_shape`
- `stage_exe_uses_extract_tool_not_installer_execution`
- `stage_exe_returns_actionable_error_when_extraction_fails`

Test snippet:
```rust
#[test]
fn stage_exe_uses_extract_tool_not_installer_execution() {
    let command = build_exe_extract_command(Path::new("C:/tmp/app.exe"), Path::new("C:/tmp/raw"));
    assert_ne!(command.get_program(), "C:/tmp/app.exe");
}
```

**Step 2: Run targeted test to verify failure**

Run:
```bash
cargo test -p crosspack-installer stage_exe_builds_extract_command_shape -- --exact
```

Expected: FAIL before command builder exists.

**Step 3: Implement minimal EXE extraction path**

In `crates/crosspack-installer/src/lib.rs`:
- Add `build_exe_extract_command(...)` using extraction tooling only.
- Add `stage_exe_payload(...)` that:
  - enforces Windows host,
  - extracts payload into `raw_dir`,
  - returns hard failure when extraction cannot start or exits non-zero.
- Do not run the EXE as an installer process.

Implementation shape:
```rust
let mut command = build_exe_extract_command(artifact_path, raw_dir);
run_command(&mut command, "failed to stage EXE artifact via deterministic extraction")
```

**Step 4: Run crate tests**

Run:
```bash
cargo test -p crosspack-installer stage_exe_ -- --nocapture
cargo test -p crosspack-installer
```

Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "feat(installer): add deterministic exe extraction staging"
```

---

### Task 4: Implement MSIX and APPX staging adapters (Windows)

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `stage_msix_builds_unpack_command_shape`
- `stage_appx_builds_unpack_command_shape`

**Step 2: Run targeted tests to verify failure**

Run:
```bash
cargo test -p crosspack-installer stage_msix_builds_unpack_command_shape -- --exact
cargo test -p crosspack-installer stage_appx_builds_unpack_command_shape -- --exact
```

Expected: FAIL before implementation.

**Step 3: Implement minimal MSIX/APPX staging**

In `crates/crosspack-installer/src/lib.rs`:
- Add `build_msix_unpack_command(...)` and `build_appx_unpack_command(...)`.
- Add `stage_msix_payload(...)` and `stage_appx_payload(...)` with Windows host guard.
- Treat both as archive/container extraction paths into `raw_dir`.

**Step 4: Run crate tests**

Run:
```bash
cargo test -p crosspack-installer stage_msix_ -- --nocapture
cargo test -p crosspack-installer stage_appx_ -- --nocapture
cargo test -p crosspack-installer
```

Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "feat(installer): add msix and appx staging adapters"
```

---

### Task 5: Implement PKG staging adapter (macOS)

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `stage_pkg_builds_expand_command_shape`
- `stage_pkg_copy_and_cleanup_command_shapes_are_stable`
- `stage_pkg_cleanup_runs_on_copy_failure`

**Step 2: Run targeted test to verify failure**

Run:
```bash
cargo test -p crosspack-installer stage_pkg_builds_expand_command_shape -- --exact
```

Expected: FAIL before command builder/hooks exist.

**Step 3: Implement minimal PKG extraction lifecycle**

In `crates/crosspack-installer/src/lib.rs`:
- Add macOS-only host guard.
- Implement expand/copy/cleanup sequence with helper hooks similar to DMG pattern.
- Ensure cleanup runs in finally-style path.
- Return actionable hard-fail errors.

**Step 4: Run crate tests**

Run:
```bash
cargo test -p crosspack-installer stage_pkg_ -- --nocapture
cargo test -p crosspack-installer
```

Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "feat(installer): add deterministic pkg staging"
```

---

### Task 6: Implement DEB staging adapter (Linux)

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `stage_deb_builds_archive_member_listing_command_shape`
- `stage_deb_selects_data_payload_member_deterministically`
- `stage_deb_rejects_missing_data_member`
- `stage_deb_builds_data_extract_command_shape`

**Step 2: Run targeted test to verify failure**

Run:
```bash
cargo test -p crosspack-installer stage_deb_selects_data_payload_member_deterministically -- --exact
```

Expected: FAIL before helper logic exists.

**Step 3: Implement minimal DEB extraction path**

In `crates/crosspack-installer/src/lib.rs`:
- Add Linux host guard.
- Implement deterministic `.deb` flow:
  1. list `ar` members,
  2. select `data.tar.*` member deterministically,
  3. extract member,
  4. unpack payload into `raw_dir`.
- Reject packages lacking data member.

Helper snippet:
```rust
fn select_deb_data_member(members: &[String]) -> Result<String> {
    // prefer exact deterministic order: data.tar.zst > .xz > .gz > .bz2 > .tar
}
```

**Step 4: Run crate tests**

Run:
```bash
cargo test -p crosspack-installer stage_deb_ -- --nocapture
cargo test -p crosspack-installer
```

Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "feat(installer): add deterministic deb staging"
```

---

### Task 7: Implement RPM staging adapter (Linux)

**Files:**
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/lib.rs`

**Step 1: Write failing tests**

Add tests:
- `stage_rpm_builds_rpm2cpio_command_shape`
- `stage_rpm_builds_cpio_extract_command_shape`
- `stage_rpm_returns_actionable_error_on_extract_failure`

**Step 2: Run targeted test to verify failure**

Run:
```bash
cargo test -p crosspack-installer stage_rpm_builds_rpm2cpio_command_shape -- --exact
```

Expected: FAIL before helper implementation.

**Step 3: Implement minimal RPM extraction path**

In `crates/crosspack-installer/src/lib.rs`:
- Add Linux host guard.
- Implement deterministic pipeline for `rpm2cpio` + `cpio` extraction into `raw_dir`.
- Do not execute `%post` or any scriptlets.
- Hard-fail with actionable message on non-zero commands.

**Step 4: Run crate tests**

Run:
```bash
cargo test -p crosspack-installer stage_rpm_ -- --nocapture
cargo test -p crosspack-installer
```

Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-installer/src/lib.rs
git commit -m "feat(installer): add deterministic rpm staging"
```

---

### Task 8: Integrate installer-kind coverage and CLI contract tests

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Test: `crates/crosspack-cli/src/main.rs`

**Step 1: Write failing tests**

Add tests:
- `install_reports_actionable_error_for_unsupported_exe_host`
- `install_reports_actionable_error_for_unsupported_pkg_host`

**Step 2: Run targeted tests to verify failure**

Run:
```bash
cargo test -p crosspack-cli install_reports_actionable_error_for_unsupported_exe_host -- --exact
```

Expected: FAIL before message assertions are aligned.

**Step 3: Implement minimal CLI alignment**

In `crates/crosspack-cli/src/main.rs`:
- Ensure install path propagates installer adapter host-guard errors unchanged or with consistent deterministic wrapping.
- Keep machine-oriented lines unchanged.

**Step 4: Run crate tests**

Run:
```bash
cargo test -p crosspack-cli
```

Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-cli/src/main.rs
git commit -m "test(cli): cover installer-kind host guard error contracts"
```

---

### Task 9: Update architecture and manifest/install docs

**Files:**
- Modify: `docs/install-flow.md`
- Modify: `docs/architecture.md`
- Modify: `docs/manifest-spec.md`

**Step 1: Update install-flow documentation**

Document:
- full artifact set: `zip`, `tar.gz`, `tar.zst`, `msi`, `dmg`, `appimage`, `exe`, `pkg`, `deb`, `rpm`, `msix`, `appx`.
- extraction-only policy for installer formats.
- host constraints for each new kind.

**Step 2: Update architecture and manifest docs**

Document:
- deterministic no-vendor-installer-execution policy,
- no maintainer-script execution for package formats,
- fail-closed behavior and actionable error semantics.

**Step 3: Commit**

Run:
```bash
git add docs/install-flow.md docs/architecture.md docs/manifest-spec.md
git commit -m "docs: describe deterministic exe pkg deb rpm msix appx staging"
```

---

### Task 10: Full verification and PR readiness

**Files:**
- Modify: none

**Step 1: Run full verification suite**

Run:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
scripts/validate-snapshot-flow.sh
```

Expected: PASS.

**Step 2: Push and monitor checks**

Run:
```bash
git push
gh pr checks --watch --interval 10
```

Expected: required checks PASS on Linux/macOS/Windows.

---

## Execution Guardrails

- Use **@superpowers:test-driven-development** for each task (failing test first, then minimal code).
- Use **@superpowers:systematic-debugging** if any test fails unexpectedly.
- Use **@superpowers:verification-before-completion** before claiming task completion.
- Preserve deterministic CLI output contracts; do not alter machine-oriented line formats.
- Keep installer ingestion fail-closed and extraction-only; never execute vendor installer UI paths.
