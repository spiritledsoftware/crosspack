# Registry Metadata Signature Verification Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fail `info/install/upgrade` when registry metadata signatures are missing or invalid, by default.

**Architecture:** Verify each manifest file's raw bytes against a detached Ed25519 signature before TOML parsing in `crosspack-registry`. Load one trusted public key from the registry root. Keep verification logic in `crosspack-security`, and keep `crosspack-cli` behavior unchanged except for clearer propagated errors.

**Tech Stack:** Rust workspace, `ed25519-dalek`, existing `anyhow`, `hex`, unit tests in crate-local `#[cfg(test)]` modules.

---

### Task 0: Isolated Workspace Setup

**Files:**
- Create: `.worktrees/feature-registry-metadata-signing/`

**Step 1: Create the worktree**

Run: `git worktree add .worktrees/feature-registry-metadata-signing -b feature/registry-metadata-signing`
Expected: New worktree created from current `main`.

**Step 2: Run baseline tests**

Run: `cargo test -p crosspack-security && cargo test -p crosspack-registry`
Expected: Baseline tests pass before feature edits.

### Task 1: Add Signature Verification Primitive in Security Crate

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/crosspack-security/Cargo.toml`
- Modify: `crates/crosspack-security/src/lib.rs`
- Test: `crates/crosspack-security/src/lib.rs`

**Step 1: Write failing tests**

Add tests for `verify_ed25519_signature_hex`:
- accepts valid signature
- rejects tampered payload (`Ok(false)`)
- errors on invalid signature hex/length
- errors on invalid public key hex/length

**Step 2: Run targeted test and verify failure**

Run: `cargo test -p crosspack-security verify_ed25519_signature_hex_accepts_valid_signature -- --exact`
Expected: FAIL because helper is not implemented.

**Step 3: Implement minimal verification helper**

Add an API in `crosspack-security`:
- `pub fn verify_ed25519_signature_hex(payload: &[u8], public_key_hex: &str, signature_hex: &str) -> Result<bool>`

Implementation notes:
- decode key/signature as hex
- parse as Ed25519 key/signature
- verify detached signature against raw payload bytes
- return `Ok(true)`/`Ok(false)` for valid parse + verify result
- return `Err` for malformed hex or invalid key/signature length

**Step 4: Run crate tests**

Run: `cargo test -p crosspack-security`
Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add Cargo.toml crates/crosspack-security/Cargo.toml crates/crosspack-security/src/lib.rs
git commit -m "feat(security): add ed25519 detached signature verification"
```

### Task 2: Enforce Strict Manifest Signature Checks in Registry Crate

**Files:**
- Modify: `crates/crosspack-registry/Cargo.toml`
- Modify: `crates/crosspack-registry/src/lib.rs`
- Test: `crates/crosspack-registry/src/lib.rs`

**Step 1: Write failing tests**

Add tests for `RegistryIndex::package_versions`:
- fails when trusted key file missing
- fails when sidecar `.sig` missing
- fails when signature invalid
- succeeds with valid signed manifest

Assumed layout:
- key file: `<registry-root>/registry.pub`
- signature file: `<registry-root>/index/<package>/<version>.toml.sig`

**Step 2: Run targeted test and verify failure**

Run: `cargo test -p crosspack-registry package_versions_requires_registry_public_key -- --exact`
Expected: FAIL before implementation.

**Step 3: Implement strict verification**

For each manifest file read in `package_versions`:
- read manifest as bytes
- read trusted key (`registry.pub`) once per call
- read detached signature sidecar for that manifest
- call `crosspack_security::verify_ed25519_signature_hex`
- fail with actionable context if key/signature file missing, malformed, or verification fails
- only parse TOML after signature verification succeeds

**Step 4: Run crate tests**

Run: `cargo test -p crosspack-registry`
Expected: PASS.

**Step 5: Commit**

Run:
```bash
git add crates/crosspack-registry/Cargo.toml crates/crosspack-registry/src/lib.rs
git commit -m "feat(registry): require signed metadata for manifest loading"
```

### Task 3: CLI Integration Safety Check

**Files:**
- Optional modify: `crates/crosspack-cli/src/main.rs`
- Optional test: `crates/crosspack-cli/src/main.rs`

**Step 1: Evaluate existing error propagation**

Run: `cargo test -p crosspack-cli`
Expected: Existing tests continue to pass if no changes needed.

**Step 2: Add minimal context only if necessary**

If registry errors are unclear at CLI boundary, wrap with `with_context` where `RegistryIndex` is used. Otherwise, skip this task to stay YAGNI.

**Step 3: Verify crate tests**

Run: `cargo test -p crosspack-cli`
Expected: PASS.

**Step 4: Commit (only if changes were made)**

```bash
git add crates/crosspack-cli/src/main.rs
git commit -m "chore(cli): improve registry signature error context"
```

### Task 4: Documentation Updates

**Files:**
- Modify: `docs/registry-spec.md`
- Modify: `docs/architecture.md`
- Modify: `docs/install-flow.md`
- Modify: `docs/manifest-spec.md`

**Step 1: Update registry spec**

Document strict metadata signing:
- required trusted key file `registry.pub`
- required detached sidecars `<version>.toml.sig`
- hex encoding requirement

**Step 2: Update architecture/install flow docs**

Document that `search/info/install/upgrade` now depend on signature-verified metadata and fail closed on signature issues.

**Step 3: Clarify manifest spec wording**

Keep artifact `signature` field semantics distinct from registry metadata sidecar signatures.

**Step 4: Commit**

```bash
git add docs/registry-spec.md docs/architecture.md docs/install-flow.md docs/manifest-spec.md
git commit -m "docs: define strict registry metadata signing behavior"
```

### Task 5: Final Verification

**Files:** none

**Step 1: Format check**

Run: `cargo fmt --all --check`
Expected: PASS.

**Step 2: Lint check**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: PASS.

**Step 3: Full test suite**

Run: `cargo test --workspace`
Expected: PASS.

**Step 4: Final commit sanity**

Run: `git log --oneline -5`
Expected: Contains task commits in order.
