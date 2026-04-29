# Installed Identity Profile Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build full installed-identity storage and selection support so same-name packages can be concurrently installed across target/profile/source namespace without receipt, sidecar, payload, pin, exposure, uninstall, or rollback collisions.

**Architecture:** Keep `crosspack-installer` as the source of truth for installed identity state, storage-owner paths, exposure-owner paths, and selector resolution. New installs write identity-keyed receipts, state documents, sidecars, pins, and package payload roots; visible projections such as `bin`, completions, GUI, services, and integrations are separately owned and conflict-checked. This follows Homebrew's payload-vs-link separation, keeps pacman/paru-style database rigor for dependency/conflict reasoning, and uses winget-style source/scope disambiguation when names are not unique enough. Legacy name-keyed paths remain readable and removable during the migration window.

**Tech Stack:** Rust workspace, Clap, serde JSON state documents, existing installer receipt/state APIs, existing CLI output renderer helpers, Cargo tests/clippy.

---

## Current Context

Already implemented on `main`:

- `crates/crosspack-installer/src/identity.rs` has `InstalledPackageIdentity { profile, target, package }` and `state_key()`.
- `crates/crosspack-installer/src/installed_state.rs` can write identity-keyed `.state.json` documents and hydrate legacy receipts/sidecars.
- `crates/crosspack-installer/src/installed_state.rs` exposes `find_installed_states_by_package_name()`.
- `crates/crosspack-installer/src/layout.rs` still exposes name-keyed receipt, sidecar, and `pkgs/<name>/<version>` package payload paths.
- `crates/crosspack-cli/src/core_flows.rs` has `resolve_unambiguous_installed_package()` that blocks ambiguous bare names, but the error says the command cannot disambiguate target/profile yet.
- `crosspack list` still renders plain `name version` lines through `format_installed_list_lines_for_style()`.
- `crosspack pin` is still name-scoped through `parse_pin_spec()` and `write_pin()`.

This plan rewrites new install storage to identity-keyed paths while preserving legacy compatibility reads. Do not ship selector support without identity-keyed payload, sidecar, and exposure ownership; that would still allow same-name installs to overwrite or delete the wrong files.

## File Structure

- Modify: `crates/crosspack-installer/src/identity.rs`
  - Add selector and match types.
  - Add source namespace and provenance dimensions with legacy defaults.
  - Add deterministic display helpers for guidance.
- Modify: `crates/crosspack-installer/src/installed_state.rs`
  - Add serde defaults for source namespace and provenance in installed-state documents.
  - Add installer-owned selector resolution API.
  - Detect duplicate identity keys when reading all states.
- Modify: `crates/crosspack-installer/src/layout.rs`
  - Add versioned identity-keyed receipt, package payload, sidecar, pin, exposure-owner, active-pointer, and state document path helpers.
  - Keep legacy name-keyed path helpers for compatibility.
- Modify: `crates/crosspack-installer/src/receipts.rs`
  - Write and parse optional `identity_*` receipt fields.
  - Add identity-keyed receipt read/write helpers.
- Modify: `crates/crosspack-installer/src/exposure.rs`
  - Add identity-keyed GUI/native/integration state helpers or adapt callers through storage-owner paths.
  - Add exposure ownership checks so multiple payloads can coexist without fighting over visible projections.
- Modify: `crates/crosspack-installer/src/uninstall.rs`
  - Remove selected identity storage owner rather than all state for a package name.
- Modify: `crates/crosspack-installer/src/lib.rs`
  - Re-export new selector/resolution types and functions.
- Modify: `crates/crosspack-installer/src/pins.rs`
  - Add identity-scoped pin path helpers and read/write APIs while preserving legacy name pins.
- Modify: `crates/crosspack-installer/src/tests.rs`
  - Add unit tests for selector matching, legacy hydration, duplicate detection, and scoped pins.
- Modify: `crates/crosspack-cli/src/main.rs`
  - Add selector flags to lifecycle commands.
  - Add `list --identity`.
- Modify: `crates/crosspack-cli/src/core_flows.rs`
  - Replace CLI-owned ambiguity lookup with installer selector API.
  - Add selector parsing helpers and pin selection helpers.
  - Install into identity-keyed package roots and write identity-keyed receipts/state.
- Modify: `crates/crosspack-cli/src/command_flows.rs`
  - Thread selectors through uninstall, upgrade, service, depends/uses/why where installed package lookup happens.
  - Capture and restore rollback snapshots by identity storage owner.
  - Keep dry-run and transaction preview contracts unchanged.
- Modify: `crates/crosspack-cli/src/dispatch.rs`
  - Wire new Clap fields into command flows.
- Modify: `crates/crosspack-cli/src/lifecycle_service.rs`
  - Add list request/outcome shape for default vs identity output.
- Modify: `crates/crosspack-cli/src/lifecycle_render.rs`
  - Render opt-in identity list lines.
- Modify: `crates/crosspack-cli/src/tests.rs`
  - Add CLI parser, rendering, ambiguity, selector, pin, and upgrade grouping tests.
- Modify: `crates/crosspack-resolver/src/plan.rs` and `crates/crosspack-resolver/src/tests.rs` only if identity-aware installed summaries need `target/profile/source_namespace` to prevent grouping regressions.
- Modify: `docs/install-flow.md` and `docs/architecture.md` after code is implemented and verified.

---

### Task 1: Extend Installed Identity Model With Source and Selectors

**Files:**
- Modify: `crates/crosspack-installer/src/identity.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing identity tests**

Add these tests near `installed_package_identity_imports_legacy_receipt_and_builds_deterministic_key` in `crates/crosspack-installer/src/tests.rs`:

```rust
#[test]
fn installed_package_identity_defaults_legacy_source_and_formats_selector() {
    let mut receipt = InstallReceipt {
        name: "demo".to_string(),
        version: "1.2.3".to_string(),
        dependencies: Vec::new(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        artifact_url: None,
        artifact_sha256: None,
        cache_path: None,
        exposed_bins: Vec::new(),
        exposed_completions: Vec::new(),
        snapshot_id: None,
        install_mode: InstallMode::Managed,
        install_reason: InstallReason::Root,
        install_status: "installed".to_string(),
        installed_at_unix: 1,
    };

    let identity = InstalledPackageIdentity::from_legacy_receipt(&receipt);
    assert_eq!(identity.profile, "default");
    assert_eq!(identity.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
    assert_eq!(identity.source_namespace, "default");
    assert_eq!(identity.source_provenance.as_deref(), Some("unknown"));
    assert_eq!(identity.package, "demo");
    assert_eq!(
        identity.selector_display(),
        "demo --target x86_64-unknown-linux-gnu --profile default --source default"
    );

    receipt.target = None;
    let host_identity = InstalledPackageIdentity::from_legacy_receipt(&receipt);
    assert_eq!(host_identity.target_label(), "host");
}

#[test]
fn installed_package_selector_matches_only_requested_dimensions() {
    let identity = InstalledPackageIdentity {
        profile: "tools".to_string(),
        target: Some("aarch64-apple-darwin".to_string()),
        source_namespace: "community".to_string(),
        source_provenance: Some("community".to_string()),
        package: "ripgrep".to_string(),
    };

    assert!(InstalledPackageSelector {
        package: "ripgrep".to_string(),
        target: None,
        profile: None,
        source_namespace: None,
    }
    .matches(&identity));
    assert!(InstalledPackageSelector {
        package: "ripgrep".to_string(),
        target: Some("aarch64-apple-darwin".to_string()),
        profile: Some("tools".to_string()),
        source_namespace: Some("community".to_string()),
    }
    .matches(&identity));
    assert!(!InstalledPackageSelector {
        package: "ripgrep".to_string(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        profile: Some("tools".to_string()),
        source_namespace: Some("community".to_string()),
    }
    .matches(&identity));
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p crosspack-installer installed_package_identity_defaults_legacy_source_and_formats_selector installed_package_selector_matches_only_requested_dimensions
```

Expected: FAIL because `source_namespace`, `source_provenance`, `target_label`, `selector_display`, and `InstalledPackageSelector` do not exist.

- [ ] **Step 3: Implement identity selector model**

Update `crates/crosspack-installer/src/identity.rs` to this shape, preserving derives:

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstalledPackageIdentity {
    pub profile: String,
    pub target: Option<String>,
    pub source_namespace: String,
    pub source_provenance: Option<String>,
    pub package: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackageSelector {
    pub package: String,
    pub target: Option<String>,
    pub profile: Option<String>,
    pub source_namespace: Option<String>,
}

impl InstalledPackageIdentity {
    pub fn from_legacy_receipt(receipt: &crate::InstallReceipt) -> Self {
        Self {
            profile: "default".to_string(),
            target: receipt.target.clone(),
            source_namespace: "default".to_string(),
            source_provenance: Some("unknown".to_string()),
            package: receipt.name.clone(),
        }
    }

    pub fn target_label(&self) -> &str {
        self.target.as_deref().unwrap_or("host")
    }

    pub fn source_namespace_label(&self) -> &str {
        &self.source_namespace
    }

    pub fn source_provenance_label(&self) -> &str {
        self.source_provenance.as_deref().unwrap_or("unknown")
    }

    pub fn state_key(&self) -> String {
        format!(
            "{}--{}--{}--{}",
            self.profile,
            self.target_label(),
            self.source_namespace_label(),
            self.package
        )
    }

    pub fn legacy_state_key(&self) -> String {
        format!("{}--{}--{}", self.profile, self.target_label(), self.package)
    }

    pub fn selector_display(&self) -> String {
        format!(
            "{} --target {} --profile {} --source {}",
            self.package,
            self.target_label(),
            self.profile,
            self.source_namespace_label()
        )
    }
}

impl InstalledPackageSelector {
    pub fn matches(&self, identity: &InstalledPackageIdentity) -> bool {
        self.package == identity.package
            && self
                .target
                .as_deref()
                .is_none_or(|target| identity.target_label() == target)
            && self
                .profile
                .as_deref()
                .is_none_or(|profile| identity.profile == profile)
            && self
                .source_namespace
                .as_deref()
                .is_none_or(|source| identity.source_namespace_label() == source)
    }
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p crosspack-installer installed_package_identity_defaults_legacy_source_and_formats_selector installed_package_selector_matches_only_requested_dimensions
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/crosspack-installer/src/identity.rs crates/crosspack-installer/src/tests.rs
git commit -m "feat(installer): add installed package selectors"
```

---

### Task 2: Preserve Installed-State Document Compatibility

**Files:**
- Modify: `crates/crosspack-installer/src/installed_state.rs`
- Modify: `crates/crosspack-installer/src/layout.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing compatibility tests**

Add this test near `installed_package_state_reads_legacy_name_keyed_document`:

```rust
#[test]
fn installed_package_state_reads_identity_document_without_source_field() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let mut receipt = InstallReceipt {
        name: "demo".to_string(),
        version: "1.0.0".to_string(),
        dependencies: Vec::new(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        artifact_url: None,
        artifact_sha256: None,
        cache_path: None,
        exposed_bins: Vec::new(),
        exposed_completions: Vec::new(),
        snapshot_id: None,
        install_mode: InstallMode::Managed,
        install_reason: InstallReason::Root,
        install_status: "installed".to_string(),
        installed_at_unix: 1,
    };
    write_install_receipt(&layout, &receipt).expect("must write receipt");

    let legacy_identity = InstalledPackageIdentity {
        profile: "default".to_string(),
        target: receipt.target.clone(),
        source_namespace: "default".to_string(),
        source_provenance: Some("unknown".to_string()),
        package: receipt.name.clone(),
    };
    let legacy_path = layout.installed_legacy_identity_state_document_path(&legacy_identity);
    let raw = r#"{
  "version": 1,
  "identity": {
    "profile": "default",
    "target": "x86_64-unknown-linux-gnu",
    "package": "demo"
  },
  "receipt": {
    "name": "demo",
    "version": "1.0.0",
    "dependencies": [],
    "target": "x86_64-unknown-linux-gnu",
    "artifact_url": null,
    "artifact_sha256": null,
    "cache_path": null,
    "exposed_bins": [],
    "exposed_completions": [],
    "snapshot_id": null,
    "install_mode": "managed",
    "install_reason": "root",
    "install_status": "installed",
    "installed_at_unix": 1
  },
  "gui_assets": [],
  "native_gui_records": [],
  "services": [],
  "integrations": []
}"#;
    fs::write(&legacy_path, raw).expect("must write legacy identity document");

    let loaded = read_installed_package_state(&layout, "demo")
        .expect("must read state")
        .expect("demo must be installed");
    assert_eq!(loaded.identity.source_namespace, "default");
    assert_eq!(loaded.identity.source_provenance.as_deref(), Some("unknown"));

    receipt.version = "1.0.1".to_string();
    let state = InstalledPackageState {
        identity: InstalledPackageIdentity::from_legacy_receipt(&receipt),
        version: receipt.version.clone(),
        receipt,
        gui_assets: Vec::new(),
        native_gui_records: Vec::new(),
        services: Vec::new(),
        integrations: Vec::new(),
    };
    let new_path = write_installed_package_state(&layout, &state).expect("must write v2 state");
    assert_ne!(new_path, legacy_path);
    assert!(new_path.exists());

    let _ = fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run failing compatibility test**

Run:

```bash
cargo test -p crosspack-installer installed_package_state_reads_identity_document_without_source_field
```

Expected: FAIL because `installed_legacy_identity_state_document_path` and serde defaults for `source_namespace` and `source_provenance` do not exist.

- [ ] **Step 3: Add compatibility path helper**

Update `crates/crosspack-installer/src/layout.rs`:

```rust
pub fn installed_legacy_identity_state_document_path(
    &self,
    identity: &InstalledPackageIdentity,
) -> PathBuf {
    self.installed_state_dir()
        .join(format!("{}.state.json", identity.legacy_state_key()))
}
```

- [ ] **Step 4: Add serde default for source and fallback read order**

Update `InstalledPackageIdentityDocument` in `installed_state.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledPackageIdentityDocument {
    profile: String,
    target: Option<String>,
    #[serde(default = "default_installed_source_namespace")]
    source_namespace: String,
    #[serde(default = "default_installed_source_provenance")]
    source_provenance: Option<String>,
    package: String,
}

fn default_installed_source_namespace() -> String {
    "default".to_string()
}

fn default_installed_source_provenance() -> Option<String> {
    Some("unknown".to_string())
}
```

Update conversions:

```rust
impl From<&InstalledPackageIdentity> for InstalledPackageIdentityDocument {
    fn from(identity: &InstalledPackageIdentity) -> Self {
        Self {
            profile: identity.profile.clone(),
            target: identity.target.clone(),
            source_namespace: identity.source_namespace.clone(),
            source_provenance: identity.source_provenance.clone(),
            package: identity.package.clone(),
        }
    }
}

impl From<InstalledPackageIdentityDocument> for InstalledPackageIdentity {
    fn from(document: InstalledPackageIdentityDocument) -> Self {
        Self {
            profile: document.profile,
            target: document.target,
            source_namespace: document.source_namespace,
            source_provenance: document
                .source_provenance
                .or_else(default_installed_source_provenance),
            package: document.package,
        }
    }
}
```

Update document lookup in `read_installed_package_state_document()` so it tries the v2 identity path, then legacy identity path, then name-keyed path:

```rust
let identity_paths = if receipt_path.exists() {
    let raw = fs::read_to_string(&receipt_path).with_context(|| {
        format!("failed to read install receipt: {}", receipt_path.display())
    })?;
    let receipt = parse_receipt(&raw).with_context(|| {
        format!("failed to parse install receipt: {}", receipt_path.display())
    })?;
    let identity = InstalledPackageIdentity::from_legacy_receipt(&receipt);
    vec![
        layout.installed_identity_state_document_path(&identity),
        layout.installed_legacy_identity_state_document_path(&identity),
    ]
} else {
    Vec::new()
};
let legacy_path = layout.installed_state_document_path(package_name);
let path = identity_paths
    .into_iter()
    .find(|path| path.exists())
    .unwrap_or(legacy_path);
```

- [ ] **Step 5: Run installer state tests**

Run:

```bash
cargo test -p crosspack-installer installed_package_state_reads_identity_document_without_source_field installed_package_state_document_round_trip_prefers_document_over_legacy_sidecars installed_package_state_reads_legacy_name_keyed_document
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/crosspack-installer/src/installed_state.rs crates/crosspack-installer/src/layout.rs crates/crosspack-installer/src/tests.rs
git commit -m "fix(installer): preserve installed state identity compatibility"
```

---

### Task 2A: Add Identity-Keyed Storage Owner Paths

**Files:**
- Modify: `crates/crosspack-installer/src/layout.rs`
- Modify: `crates/crosspack-installer/src/receipts.rs`
- Modify: `crates/crosspack-installer/src/types.rs` if a shared storage-owner struct belongs there.
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing storage path tests**

Add these tests near the transaction/layout path tests in `crates/crosspack-installer/src/tests.rs`:

```rust
#[test]
fn identity_storage_paths_do_not_collide_for_same_name_and_version() {
    let layout = test_layout();
    let linux = InstalledPackageIdentity {
        profile: "default".to_string(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        source_namespace: "community".to_string(),
        source_provenance: Some("community".to_string()),
        package: "demo".to_string(),
    };
    let macos = InstalledPackageIdentity {
        profile: "default".to_string(),
        target: Some("aarch64-apple-darwin".to_string()),
        source_namespace: "community".to_string(),
        source_provenance: Some("community".to_string()),
        package: "demo".to_string(),
    };

    assert_ne!(
        layout.identity_package_dir(&linux, "1.0.0"),
        layout.identity_package_dir(&macos, "1.0.0")
    );
    assert_ne!(layout.identity_receipt_path(&linux), layout.identity_receipt_path(&macos));
    assert_ne!(layout.identity_gui_state_path(&linux), layout.identity_gui_state_path(&macos));
    assert_ne!(layout.identity_integration_state_path(&linux), layout.identity_integration_state_path(&macos));
}

#[test]
fn identity_receipt_round_trip_preserves_identity_fields() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let receipt = InstallReceipt {
        name: "demo".to_string(),
        version: "1.0.0".to_string(),
        dependencies: Vec::new(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        artifact_url: None,
        artifact_sha256: None,
        cache_path: None,
        exposed_bins: Vec::new(),
        exposed_completions: Vec::new(),
        snapshot_id: None,
        install_mode: InstallMode::Managed,
        install_reason: InstallReason::Root,
        install_status: "installed".to_string(),
        installed_at_unix: 1,
    };
    let identity = InstalledPackageIdentity {
        profile: "default".to_string(),
        target: receipt.target.clone(),
        source_namespace: "community".to_string(),
        source_provenance: Some("community".to_string()),
        package: receipt.name.clone(),
    };

    write_identity_install_receipt(&layout, &identity, &receipt)
        .expect("must write identity receipt");
    let loaded = read_identity_install_receipt(&layout, &identity)
        .expect("must read identity receipt")
        .expect("receipt must exist");

    assert_eq!(loaded.receipt.name, "demo");
    assert_eq!(loaded.identity, identity);

    let _ = fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p crosspack-installer identity_storage_paths_do_not_collide_for_same_name_and_version identity_receipt_round_trip_preserves_identity_fields
```

Expected: FAIL because identity storage path and receipt APIs do not exist.

- [ ] **Step 3: Add identity storage path helpers**

Add to `PrefixLayout` in `layout.rs`:

```rust
pub fn identity_pkgs_dir(&self) -> PathBuf {
    self.pkgs_dir().join("identities").join("v1")
}

pub fn identity_package_dir(
    &self,
    identity: &InstalledPackageIdentity,
    version: &str,
) -> PathBuf {
    self.identity_pkgs_dir().join(identity.state_key()).join(version)
}

pub fn identity_receipt_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
    self.installed_state_dir()
        .join(format!("{}.receipt", identity.state_key()))
}

pub fn identity_gui_state_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
    self.installed_state_dir()
        .join(format!("{}.gui", identity.state_key()))
}

pub fn identity_gui_native_state_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
    self.installed_state_dir()
        .join(format!("{}.gui-native", identity.state_key()))
}

pub fn identity_declared_services_state_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
    self.installed_state_dir()
        .join(format!("{}.services", identity.state_key()))
}

pub fn identity_integration_state_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
    self.installed_state_dir()
        .join(format!("{}.integrations", identity.state_key()))
}
```

Add `self.identity_pkgs_dir()` to `ensure_base_dirs()`.

- [ ] **Step 4: Add identity receipt read/write APIs**

Create a small returned pair in `receipts.rs` or `types.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityInstallReceipt {
    pub identity: InstalledPackageIdentity,
    pub receipt: InstallReceipt,
}
```

Update receipt write payload to include identity fields when using identity writes:

```rust
pub fn write_identity_install_receipt(
    layout: &PrefixLayout,
    identity: &InstalledPackageIdentity,
    receipt: &InstallReceipt,
) -> Result<PathBuf> {
    let path = layout.identity_receipt_path(identity);
    let payload = format_install_receipt_payload(Some(identity), receipt);
    crate::atomic_write::write_file_atomically(&path, payload.as_bytes())?;
    Ok(path)
}
```

Refactor existing `write_install_receipt()` to call:

```rust
fn format_install_receipt_payload(
    identity: Option<&InstalledPackageIdentity>,
    receipt: &InstallReceipt,
) -> String
```

When `identity` is present, include:

```rust
payload.push_str(&format!("identity_profile={}\n", identity.profile));
if let Some(target) = &identity.target {
    payload.push_str(&format!("identity_target={target}\n"));
}
payload.push_str(&format!("identity_source_namespace={}\n", identity.source_namespace));
if let Some(source_provenance) = &identity.source_provenance {
    payload.push_str(&format!("identity_source_provenance={source_provenance}\n"));
}
payload.push_str(&format!("identity_package={}\n", identity.package));
```

Add parsing support for `identity_profile`, `identity_target`, `identity_source_namespace`, `identity_source_provenance`, and `identity_package`. Accept legacy `identity_source` as provenance only. Keep `parse_receipt(raw)` returning `InstallReceipt`; add:

```rust
pub fn parse_identity_receipt(raw: &str) -> Result<IdentityInstallReceipt>
```

For legacy receipts with no identity fields, hydrate `InstalledPackageIdentity::from_legacy_receipt(&receipt)`.

- [ ] **Step 5: Re-export identity receipt APIs**

Update `lib.rs` to export:

```rust
read_identity_install_receipt, write_identity_install_receipt, IdentityInstallReceipt
```

- [ ] **Step 6: Run storage tests**

Run:

```bash
cargo test -p crosspack-installer identity_storage_paths_do_not_collide_for_same_name_and_version identity_receipt_round_trip_preserves_identity_fields parse_old_receipt_shape parse_new_receipt_shape
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/crosspack-installer/src/layout.rs crates/crosspack-installer/src/receipts.rs crates/crosspack-installer/src/types.rs crates/crosspack-installer/src/lib.rs crates/crosspack-installer/src/tests.rs
git commit -m "feat(installer): add identity-keyed install storage paths"
```

---

### Task 3: Add Installer-Owned Selector Resolution

**Files:**
- Modify: `crates/crosspack-installer/src/installed_state.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing selector resolution tests**

Add tests near `find_installed_states_by_package_name_returns_all_matching_identities`:

```rust
#[test]
fn resolve_installed_package_selector_returns_exact_match() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let mut linux = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
    linux.target = Some("x86_64-unknown-linux-gnu".to_string());
    let linux_state = InstalledPackageState {
        identity: InstalledPackageIdentity::from_legacy_receipt(&linux),
        version: linux.version.clone(),
        receipt: linux,
        gui_assets: Vec::new(),
        native_gui_records: Vec::new(),
        services: Vec::new(),
        integrations: Vec::new(),
    };
    write_installed_package_state(&layout, &linux_state).expect("must write linux state");

    let mut macos = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
    macos.target = Some("aarch64-apple-darwin".to_string());
    let macos_state = InstalledPackageState {
        identity: InstalledPackageIdentity::from_legacy_receipt(&macos),
        version: macos.version.clone(),
        receipt: macos,
        gui_assets: Vec::new(),
        native_gui_records: Vec::new(),
        services: Vec::new(),
        integrations: Vec::new(),
    };
    write_installed_package_state(&layout, &macos_state).expect("must write macos state");

    let selected = resolve_installed_package_selector(
        &layout,
        &InstalledPackageSelector {
            package: "demo".to_string(),
            target: Some("aarch64-apple-darwin".to_string()),
            profile: Some("default".to_string()),
            source_namespace: Some("default".to_string()),
        },
    )
    .expect("selector resolution should succeed")
    .expect("selector should match");

    assert_eq!(selected.identity.target.as_deref(), Some("aarch64-apple-darwin"));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn resolve_installed_package_selector_reports_sorted_ambiguity() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
        let mut receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
        receipt.target = Some(target.to_string());
        let state = InstalledPackageState {
            identity: InstalledPackageIdentity::from_legacy_receipt(&receipt),
            version: receipt.version.clone(),
            receipt,
            gui_assets: Vec::new(),
            native_gui_records: Vec::new(),
            services: Vec::new(),
            integrations: Vec::new(),
        };
        write_installed_package_state(&layout, &state).expect("must write state");
    }

    let err = resolve_installed_package_selector(
        &layout,
        &InstalledPackageSelector {
            package: "demo".to_string(),
            target: None,
            profile: None,
            source_namespace: None,
        },
    )
    .expect_err("bare selector must be ambiguous");

    assert_eq!(err.matches.len(), 2);
    assert_eq!(err.matches[0].identity.target.as_deref(), Some("aarch64-apple-darwin"));
    assert_eq!(err.matches[1].identity.target.as_deref(), Some("x86_64-unknown-linux-gnu"));

    let _ = fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p crosspack-installer resolve_installed_package_selector
```

Expected: FAIL because the resolver API and error type do not exist.

- [ ] **Step 3: Implement resolver API**

Add to `installed_state.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackageSelectionAmbiguity {
    pub selector: InstalledPackageSelector,
    pub matches: Vec<InstalledPackageState>,
}

impl std::fmt::Display for InstalledPackageSelectionAmbiguity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "installed package name '{}' is ambiguous",
            self.selector.package
        )
    }
}

impl std::error::Error for InstalledPackageSelectionAmbiguity {}

pub fn resolve_installed_package_selector(
    layout: &PrefixLayout,
    selector: &InstalledPackageSelector,
) -> std::result::Result<Option<InstalledPackageState>, InstalledPackageSelectionAmbiguity> {
    let mut matches = read_all_installed_package_states(layout)
        .map_err(|err| InstalledPackageSelectionAmbiguity {
            selector: selector.clone(),
            matches: vec![InstalledPackageState {
                identity: InstalledPackageIdentity {
                    profile: "error".to_string(),
                    target: None,
                    source_namespace: "default".to_string(),
                    source_provenance: Some("unknown".to_string()),
                    package: format!("state-read-error:{err}"),
                },
                version: "0.0.0".to_string(),
                receipt: InstallReceipt {
                    name: selector.package.clone(),
                    version: "0.0.0".to_string(),
                    dependencies: Vec::new(),
                    target: None,
                    artifact_url: None,
                    artifact_sha256: None,
                    cache_path: None,
                    exposed_bins: Vec::new(),
                    exposed_completions: Vec::new(),
                    snapshot_id: None,
                    install_mode: InstallMode::Managed,
                    install_reason: InstallReason::Dependency,
                    install_status: "error".to_string(),
                    installed_at_unix: 0,
                },
                gui_assets: Vec::new(),
                native_gui_records: Vec::new(),
                services: Vec::new(),
                integrations: Vec::new(),
            }],
        })?
        .into_iter()
        .filter(|state| selector.matches(&state.identity))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.identity.cmp(&right.identity));

    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.into_iter().next()),
        _ => Err(InstalledPackageSelectionAmbiguity {
            selector: selector.clone(),
            matches,
        }),
    }
}
```

Before committing, simplify the above error handling to preserve IO errors as `anyhow::Error` instead of embedding a synthetic state. The final signature should be:

```rust
pub fn resolve_installed_package_selector(
    layout: &PrefixLayout,
    selector: &InstalledPackageSelector,
) -> Result<std::result::Result<Option<InstalledPackageState>, InstalledPackageSelectionAmbiguity>>
```

Implementation body:

```rust
let mut matches = read_all_installed_package_states(layout)?
    .into_iter()
    .filter(|state| selector.matches(&state.identity))
    .collect::<Vec<_>>();
matches.sort_by(|left, right| left.identity.cmp(&right.identity));

Ok(match matches.len() {
    0 => Ok(None),
    1 => Ok(matches.into_iter().next()),
    _ => Err(InstalledPackageSelectionAmbiguity {
        selector: selector.clone(),
        matches,
    }),
})
```

- [ ] **Step 4: Re-export new types**

Update `crates/crosspack-installer/src/lib.rs`:

```rust
pub use identity::{InstalledPackageIdentity, InstalledPackageSelector};
pub use installed_state::{
    find_installed_states_by_package_name, read_all_installed_package_states,
    read_installed_package_state, resolve_installed_package_selector,
    write_installed_package_state, InstalledPackageSelectionAmbiguity, InstalledPackageState,
};
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p crosspack-installer resolve_installed_package_selector
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/crosspack-installer/src/installed_state.rs crates/crosspack-installer/src/lib.rs crates/crosspack-installer/src/tests.rs
git commit -m "feat(installer): resolve installed package selectors"
```

---

### Task 3A: Route Install, Sidecars, and Uninstall Through Identity Storage

**Files:**
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Modify: `crates/crosspack-installer/src/exposure.rs`
- Modify: `crates/crosspack-installer/src/receipts.rs`
- Modify: `crates/crosspack-installer/src/uninstall.rs`
- Modify: `crates/crosspack-installer/src/installed_state.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Test: `crates/crosspack-cli/src/tests.rs`
- Test: `crates/crosspack-installer/src/tests.rs`

- [ ] **Step 1: Write failing concurrent install storage test**

Add a CLI test near `install_resolved_writes_legacy_receipt_and_state_document`:

```rust
#[test]
fn install_resolved_allows_same_name_same_version_for_different_targets() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let mut linux = resolved_install("demo-bin", "1.0.0");
    linux.resolved_target = "x86_64-unknown-linux-gnu".to_string();
    seed_cached_artifact(&layout, &linux, b"#!/bin/sh\n");
    let linux_plan = build_install_plan_from_resolved(
        PlanOperation::Install,
        Some(linux.resolved_target.clone()),
        std::slice::from_ref(&linux),
        &[],
        &[RootInstallRequest { name: "demo-bin".to_string(), requirement: VersionReq::STAR }],
    );
    install_resolved(
        &layout,
        &linux,
        &[],
        InstallResolvedPlanContext {
            root_names: &["demo-bin".to_string()],
            install_plan: &linux_plan,
            planned_dependency_overrides: &HashMap::new(),
        },
        InstallResolvedOptions {
            snapshot_id: None,
            force_redownload: false,
            interaction_policy: InstallInteractionPolicy::default(),
            install_progress_mode: InstallProgressMode::Disabled,
        },
        None,
    )
    .expect("linux install should succeed");

    let mut macos = resolved_install("demo-bin", "1.0.0");
    macos.resolved_target = "aarch64-apple-darwin".to_string();
    seed_cached_artifact(&layout, &macos, b"#!/bin/sh\n");
    let macos_plan = build_install_plan_from_resolved(
        PlanOperation::Install,
        Some(macos.resolved_target.clone()),
        std::slice::from_ref(&macos),
        &[],
        &[RootInstallRequest { name: "demo-bin".to_string(), requirement: VersionReq::STAR }],
    );
    install_resolved(
        &layout,
        &macos,
        &[],
        InstallResolvedPlanContext {
            root_names: &["demo-bin".to_string()],
            install_plan: &macos_plan,
            planned_dependency_overrides: &HashMap::new(),
        },
        InstallResolvedOptions {
            snapshot_id: None,
            force_redownload: false,
            interaction_policy: InstallInteractionPolicy::default(),
            install_progress_mode: InstallProgressMode::Disabled,
        },
        None,
    )
    .expect("macos install should not overwrite linux install");

    let states = find_installed_states_by_package_name(&layout, "demo-bin")
        .expect("must read states");
    assert_eq!(states.len(), 2);
    assert_ne!(
        layout.identity_package_dir(&states[0].identity, &states[0].receipt.version),
        layout.identity_package_dir(&states[1].identity, &states[1].receipt.version)
    );

    let _ = std::fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run failing test**

Run:

```bash
cargo test -p crosspack-cli install_resolved_allows_same_name_same_version_for_different_targets
```

Expected: FAIL because install still writes name-keyed receipt and payload paths.

- [ ] **Step 3: Add storage owner type**

Add to installer state/types:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledStorageKind {
    LegacyName,
    Identity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledStorageOwner {
    pub identity: InstalledPackageIdentity,
    pub kind: InstalledStorageKind,
}
```

Add `storage: InstalledStorageOwner` or `storage_kind: InstalledStorageKind` to `InstalledPackageState`. Legacy hydration sets `LegacyName`; identity receipt/state reads set `Identity`.

- [ ] **Step 4: Make install use identity package roots**

In `install_resolved()`, compute identity before staging package files:

```rust
let identity = InstalledPackageIdentity {
    profile: "default".to_string(),
    target: Some(resolved.resolved_target.clone()),
    source_namespace: "default".to_string(),
    source_provenance: Some("unknown".to_string()),
    package: resolved.manifest.name.clone(),
};
let package_dir = layout.identity_package_dir(&identity, &resolved.manifest.version.to_string());
```

Replace uses of `layout.package_dir(&resolved.manifest.name, ...)` in the install apply path with this identity package dir. Keep cache paths unchanged.

- [ ] **Step 5: Make install write identity receipts and sidecars**

Replace install receipt write:

```rust
let receipt_path = write_identity_install_receipt(layout, &identity, &receipt)?;
```

Write installed package state using the same identity and mark storage owner as identity-backed. Adapt GUI/native/service/integration state writes to identity-keyed sidecar paths. If the existing sidecar APIs accept only `package_name`, add identity variants rather than overloading package name with a state key.

- [ ] **Step 6: Add identity-aware uninstall API**

In `uninstall.rs`, add:

```rust
pub fn uninstall_package_identity(
    layout: &PrefixLayout,
    identity: &InstalledPackageIdentity,
) -> Result<UninstallResult>
```

It must:

- read the identity receipt,
- remove only `layout.identity_package_dir(identity, &receipt.version)`,
- remove only identity-keyed receipt/state/sidecars,
- remove exposed bins/completions owned by that identity,
- run dependency blocking against remaining installed identities.

Keep `uninstall_package(layout, name)` as the legacy compatibility path.

- [ ] **Step 7: Run storage and uninstall tests**

Run:

```bash
cargo test -p crosspack-cli install_resolved_allows_same_name_same_version_for_different_targets
cargo test -p crosspack-installer uninstall_removes_package_dir_and_receipt
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/crosspack-cli/src/core_flows.rs crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/tests.rs crates/crosspack-installer/src/exposure.rs crates/crosspack-installer/src/receipts.rs crates/crosspack-installer/src/uninstall.rs crates/crosspack-installer/src/installed_state.rs crates/crosspack-installer/src/lib.rs crates/crosspack-installer/src/tests.rs
git commit -m "feat(installer): install packages into identity-keyed storage"
```

---

### Task 4: Add CLI Selector Parsing and Flags

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing parser and Clap tests**

Add tests near existing CLI parser tests:

```rust
#[test]
fn cli_parses_uninstall_identity_selector_flags() {
    let cli = Cli::try_parse_from([
        "crosspack",
        "uninstall",
        "demo",
        "--target",
        "aarch64-apple-darwin",
        "--profile",
        "tools",
        "--source",
        "community",
    ])
    .expect("uninstall selector flags should parse");

    match cli.command {
        Commands::Uninstall {
            name,
            target,
            profile,
            source,
            ..
        } => {
            assert_eq!(name, "demo");
            assert_eq!(target.as_deref(), Some("aarch64-apple-darwin"));
            assert_eq!(profile.as_deref(), Some("tools"));
            assert_eq!(source.as_deref(), Some("community"));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_installed_package_selector_accepts_compact_target_profile_syntax() {
    let selector = parse_installed_package_selector(
        "ripgrep@x86_64-unknown-linux-gnu#tools",
        None,
        None,
        None,
    )
    .expect("compact selector should parse");

    assert_eq!(selector.package, "ripgrep");
    assert_eq!(selector.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
    assert_eq!(selector.profile.as_deref(), Some("tools"));
    assert_eq!(selector.source_namespace, None);
}

#[test]
fn parse_installed_package_selector_rejects_conflicting_target_sources() {
    let err = parse_installed_package_selector(
        "ripgrep@x86_64-unknown-linux-gnu",
        Some("aarch64-apple-darwin".to_string()),
        None,
        None,
    )
    .expect_err("conflicting target selector should fail");

    assert!(err.to_string().contains("target specified twice"));
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p crosspack-cli cli_parses_uninstall_identity_selector_flags parse_installed_package_selector
```

Expected: FAIL because the flags and parser do not exist.

- [ ] **Step 3: Add fields to `Commands::Uninstall`**

Update `main.rs`:

```rust
Uninstall {
    name: String,
    #[arg(long)]
    target: Option<String>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    source: Option<String>,
    #[command(flatten)]
    escalation: EscalationArgs,
},
```

- [ ] **Step 4: Add selector parser helper**

Add to `core_flows.rs` near `parse_spec()`:

```rust
fn parse_installed_package_selector(
    raw: &str,
    target_flag: Option<String>,
    profile_flag: Option<String>,
    source_flag: Option<String>,
) -> Result<InstalledPackageSelector> {
    let (name_and_target, compact_profile) = match raw.split_once('#') {
        Some((left, right)) if !right.is_empty() => (left, Some(right.to_string())),
        Some(_) => return Err(anyhow!("profile selector must not be empty")),
        None => (raw, None),
    };
    let (package, compact_target) = match name_and_target.split_once('@') {
        Some((left, right)) if !left.is_empty() && !right.is_empty() => {
            (left.to_string(), Some(right.to_string()))
        }
        Some(_) => return Err(anyhow!("target selector must include package and target")),
        None => (name_and_target.to_string(), None),
    };
    if package.trim().is_empty() {
        return Err(anyhow!("package selector must not be empty"));
    }
    if compact_target.is_some() && target_flag.is_some() {
        return Err(anyhow!("target specified twice"));
    }
    if compact_profile.is_some() && profile_flag.is_some() {
        return Err(anyhow!("profile specified twice"));
    }

    Ok(InstalledPackageSelector {
        package,
        target: target_flag.or(compact_target),
        profile: profile_flag.or(compact_profile),
        source_namespace: source_flag,
    })
}
```

- [ ] **Step 5: Wire dispatch compile path**

Update `dispatch.rs` uninstall match to include ignored selector fields temporarily:

```rust
Commands::Uninstall {
    name,
    target,
    profile,
    source,
    escalation,
} => {
    let _escalation_policy = resolve_escalation_policy(escalation);
    let prefix = default_user_prefix()?;
    let layout = PrefixLayout::new(prefix);
    run_uninstall_command_with_selector(&layout, name, target, profile, source)?;
}
```

Add a temporary wrapper in `command_flows.rs`:

```rust
fn run_uninstall_command_with_selector(
    layout: &PrefixLayout,
    name: String,
    target: Option<String>,
    profile: Option<String>,
    source: Option<String>,
) -> Result<()> {
    let selector = parse_installed_package_selector(&name, target, profile, source)?;
    run_uninstall_command(layout, selector.package)
}
```

This keeps behavior unchanged until Task 5 replaces the lookup.

- [ ] **Step 6: Run parser tests**

Run:

```bash
cargo test -p crosspack-cli cli_parses_uninstall_identity_selector_flags parse_installed_package_selector
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/crosspack-cli/src/main.rs crates/crosspack-cli/src/core_flows.rs crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/dispatch.rs crates/crosspack-cli/src/tests.rs
git commit -m "feat(cli): parse installed package selectors"
```

---

### Task 5: Use Installer Selector Resolution for Identity-Scoped Uninstall and Upgrade

**Files:**
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing selector behavior tests**

Update `ambiguous_installed_package_name_blocks_uninstall_with_identity_guidance` to expect actionable selectors, not `cannot disambiguate target/profile yet`:

```rust
assert!(
    message.contains("installed package name 'demo' is ambiguous; specify one of:"),
    "unexpected error: {message}"
);
assert!(
    message.contains("demo --target aarch64-apple-darwin --profile default --source default")
        && message.contains("demo --target x86_64-unknown-linux-gnu --profile default --source default"),
    "error should list matching selectors: {message}"
);
```

Add a new identity-scoped uninstall selector test:

```rust
#[test]
fn uninstall_selector_target_removes_only_matching_identity_storage() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
        let mut receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
        receipt.target = Some(target.to_string());
        let identity = InstalledPackageIdentity::from_legacy_receipt(&receipt);
        let state = InstalledPackageState {
            identity: identity.clone(),
            version: receipt.version.clone(),
            receipt: receipt.clone(),
            gui_assets: Vec::new(),
            native_gui_records: Vec::new(),
            services: Vec::new(),
            integrations: Vec::new(),
        };
        fs::create_dir_all(layout.identity_package_dir(&identity, &receipt.version))
            .expect("must create identity package dir");
        write_identity_install_receipt(&layout, &identity, &receipt)
            .expect("must write identity receipt");
        write_installed_package_state(&layout, &state).expect("must write state");
    }

    run_uninstall_command_with_selector(
        &layout,
        "demo".to_string(),
        Some("aarch64-apple-darwin".to_string()),
        None,
        None,
    )
    .expect("target selector should remove only selected identity");

    let remaining = find_installed_states_by_package_name(&layout, "demo")
        .expect("must read remaining states");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].identity.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
    assert!(layout
        .identity_package_dir(&remaining[0].identity, &remaining[0].receipt.version)
        .exists());

    let _ = std::fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p crosspack-cli ambiguous_installed_package_name_blocks_uninstall_with_identity_guidance uninstall_selector_target_removes_only_matching_identity_storage
```

Expected: FAIL because CLI still uses old ambiguity message and uninstall still removes by package name only.

- [ ] **Step 3: Replace CLI ambiguity helper with installer resolver**

Replace `resolve_unambiguous_installed_package()` in `core_flows.rs` with:

```rust
fn resolve_installed_selector_for_cli(
    layout: &PrefixLayout,
    selector: &InstalledPackageSelector,
) -> Result<Option<InstalledPackageState>> {
    match resolve_installed_package_selector(layout, selector)? {
        Ok(state) => Ok(state),
        Err(ambiguity) => {
            let choices = ambiguity
                .matches
                .iter()
                .map(|state| format!("  {}", state.identity.selector_display()))
                .collect::<Vec<_>>()
                .join("\n");
            Err(anyhow!(
                "installed package name '{}' is ambiguous; specify one of:\n{}",
                ambiguity.selector.package,
                choices
            ))
        }
    }
}
```

- [ ] **Step 4: Make uninstall use selected state**

Change `run_uninstall_command_with_selector()` to resolve the selector and call an identity-aware uninstall API:

```rust
fn run_uninstall_command_with_selector(
    layout: &PrefixLayout,
    name: String,
    target: Option<String>,
    profile: Option<String>,
    source: Option<String>,
) -> Result<()> {
    let selector = parse_installed_package_selector(&name, target, profile, source)?;
    let Some(installed_state) = resolve_installed_selector_for_cli(layout, &selector)? else {
        println!("{} is not installed", selector.package);
        return Ok(());
    };
    run_uninstall_command_for_identity(layout, installed_state.identity)
}
```

Add `run_uninstall_command_for_identity()` next to `run_uninstall_command()`. It should use `uninstall_package_identity(layout, &identity)` for identity-keyed installs and retain legacy `uninstall_package(layout, &name)` only when the selected state has legacy storage ownership.

- [ ] **Step 5: Thread selector into single-package upgrade**

Replace calls to `resolve_unambiguous_installed_package(layout, &name)` in both dry-run and apply single-upgrade paths with selector resolution for the bare package name:

```rust
let selector = InstalledPackageSelector {
    package: name.clone(),
    target: None,
    profile: None,
    source_namespace: None,
};
let Some(installed_state) = resolve_installed_selector_for_cli(layout, &selector)? else {
    println!("{name} is not installed");
    return Ok(());
};
```

- [ ] **Step 6: Run focused CLI tests**

Run:

```bash
cargo test -p crosspack-cli ambiguous_installed_package_name_blocks_uninstall_with_identity_guidance uninstall_selector_target_removes_only_matching_identity_storage run_upgrade_command_reports_preflight_context_when_transaction_active
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/crosspack-cli/src/core_flows.rs crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/tests.rs
git commit -m "feat(cli): resolve lifecycle commands by installed identity"
```

---

### Task 6: Add Opt-In Identity List Output

**Files:**
- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Modify: `crates/crosspack-cli/src/lifecycle_service.rs`
- Modify: `crates/crosspack-cli/src/lifecycle_render.rs`
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing list output tests**

Add near `render_list_command_outcome_preserves_receipt_order_and_plain_output`:

```rust
#[test]
fn render_list_command_outcome_identity_mode_includes_identity_fields() {
    let receipt = InstallReceipt {
        name: "demo".to_string(),
        version: "1.0.0".to_string(),
        dependencies: Vec::new(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        artifact_url: None,
        artifact_sha256: None,
        cache_path: None,
        exposed_bins: Vec::new(),
        exposed_completions: Vec::new(),
        snapshot_id: None,
        install_mode: InstallMode::Managed,
        install_reason: InstallReason::Root,
        install_status: "installed".to_string(),
        installed_at_unix: 1,
    };
    let state = InstalledPackageState {
        identity: InstalledPackageIdentity::from_legacy_receipt(&receipt),
        version: receipt.version.clone(),
        receipt,
        gui_assets: Vec::new(),
        native_gui_records: Vec::new(),
        services: Vec::new(),
        integrations: Vec::new(),
    };

    let outcome = build_list_command_outcome(vec![state], ListOutputMode::Identity);
    assert_eq!(
        render_list_command_outcome(outcome),
        vec!["demo 1.0.0 target=x86_64-unknown-linux-gnu profile=default source=default".to_string()]
    );
}
```

Add parser test:

```rust
#[test]
fn cli_parses_list_identity_flag() {
    let cli = Cli::try_parse_from(["crosspack", "list", "--identity"])
        .expect("list identity flag should parse");
    match cli.command {
        Commands::List { identity } => assert!(identity),
        other => panic!("unexpected command: {other:?}"),
    }
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p crosspack-cli render_list_command_outcome_identity_mode_includes_identity_fields cli_parses_list_identity_flag
```

Expected: FAIL because `ListOutputMode` and `--identity` do not exist.

- [ ] **Step 3: Add list mode types**

Update `lifecycle_service.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListOutputMode {
    Default,
    Identity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListCommandRequest {
    mode: ListOutputMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListCommandOutcome {
    states: Vec<InstalledPackageState>,
    mode: ListOutputMode,
}

fn build_list_command_outcome(
    mut states: Vec<InstalledPackageState>,
    mode: ListOutputMode,
) -> ListCommandOutcome {
    states.sort_by(|left, right| left.identity.cmp(&right.identity));
    ListCommandOutcome { states, mode }
}
```

- [ ] **Step 4: Add render helper**

Update `lifecycle_render.rs`:

```rust
fn render_list_command_outcome(outcome: ListCommandOutcome) -> Vec<String> {
    match outcome.mode {
        ListOutputMode::Default => {
            let receipts = outcome
                .states
                .into_iter()
                .map(|state| state.receipt)
                .collect::<Vec<_>>();
            format_installed_list_lines_for_style(current_output_style(), &receipts)
        }
        ListOutputMode::Identity => format_installed_identity_list_lines(current_output_style(), &outcome.states),
    }
}
```

Add to `command_flows.rs` near `format_installed_list_lines_for_style()`:

```rust
fn format_installed_identity_list_lines(
    style: OutputStyle,
    states: &[InstalledPackageState],
) -> Vec<String> {
    if states.is_empty() {
        return render_empty_state(
            style,
            "No installed packages",
            Some("Run `crosspack install <name>` to install a package."),
        );
    }
    if style == OutputStyle::Plain {
        return states
            .iter()
            .map(|state| {
                format!(
                    "{} {} target={} profile={} source={}",
                    state.receipt.name,
                    state.receipt.version,
                    state.identity.target_label(),
                    state.identity.profile,
                    state.identity.source_namespace_label()
                )
            })
            .collect();
    }
    let mut rows = vec![
        vec!["name".to_string(), "version".to_string(), "target".to_string(), "profile".to_string(), "source".to_string()],
    ];
    for state in states {
        rows.push(vec![
            state.receipt.name.clone(),
            state.receipt.version.clone(),
            state.identity.target_label().to_string(),
            state.identity.profile.clone(),
            state.identity.source_namespace_label().to_string(),
        ]);
    }
    render_compact_table(style, &rows)
}
```

- [ ] **Step 5: Add Clap flag and dispatch wiring**

Update `main.rs`:

```rust
List {
    #[arg(long)]
    identity: bool,
},
```

Update `dispatch.rs`:

```rust
Commands::List { identity } => {
    let _request = ListCommandRequest {
        mode: if identity {
            ListOutputMode::Identity
        } else {
            ListOutputMode::Default
        },
    };
    let prefix = default_user_prefix()?;
    let layout = PrefixLayout::new(prefix);
    let states = read_all_installed_package_states(&layout)?;
    let outcome = build_list_command_outcome(states, _request.mode);
    for line in render_list_command_outcome(outcome) {
        println!("{line}");
    }
}
```

- [ ] **Step 6: Run list tests**

Run:

```bash
cargo test -p crosspack-cli render_list_command_outcome_identity_mode_includes_identity_fields render_list_command_outcome_preserves_receipt_order_and_plain_output cli_parses_list_identity_flag
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/crosspack-cli/src/main.rs crates/crosspack-cli/src/dispatch.rs crates/crosspack-cli/src/lifecycle_service.rs crates/crosspack-cli/src/lifecycle_render.rs crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/tests.rs
git commit -m "feat(cli): add identity-aware list output"
```

---

### Task 7: Add Identity-Scoped Pins With Legacy Fallback

**Files:**
- Modify: `crates/crosspack-installer/src/pins.rs`
- Modify: `crates/crosspack-installer/src/layout.rs`
- Modify: `crates/crosspack-installer/src/lib.rs`
- Modify: `crates/crosspack-cli/src/main.rs`
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Test: `crates/crosspack-installer/src/tests.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing installer pin tests**

Add to installer tests near pin tests:

```rust
#[test]
fn identity_scoped_pin_round_trip_does_not_replace_legacy_pin() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let identity = InstalledPackageIdentity {
        profile: "default".to_string(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        source_namespace: "default".to_string(),
        source_provenance: Some("unknown".to_string()),
        package: "demo".to_string(),
    };

    write_pin(&layout, "demo", "<2.0.0").expect("must write legacy pin");
    let scoped_path = write_identity_pin(&layout, &identity, "=1.5.0").expect("must write scoped pin");

    assert_eq!(read_pin(&layout, "demo").expect("must read legacy pin"), Some("<2.0.0".to_string()));
    assert_eq!(read_identity_pin(&layout, &identity).expect("must read scoped pin"), Some("=1.5.0".to_string()));
    assert!(scoped_path.ends_with("default--x86_64-unknown-linux-gnu--default--demo.pin"));

    let _ = fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run failing installer pin test**

Run:

```bash
cargo test -p crosspack-installer identity_scoped_pin_round_trip_does_not_replace_legacy_pin
```

Expected: FAIL because identity pin APIs do not exist.

- [ ] **Step 3: Implement identity pin APIs**

Add to `layout.rs`:

```rust
pub fn identity_pin_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
    self.pins_dir().join(format!("{}.pin", identity.state_key()))
}
```

Add to `pins.rs`:

```rust
pub fn write_identity_pin(
    layout: &PrefixLayout,
    identity: &InstalledPackageIdentity,
    requirement: &str,
) -> Result<PathBuf> {
    let path = layout.identity_pin_path(identity);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create pins dir: {}", parent.display()))?;
    }
    crate::atomic_write::write_file_atomically(&path, requirement.as_bytes())?;
    Ok(path)
}

pub fn read_identity_pin(
    layout: &PrefixLayout,
    identity: &InstalledPackageIdentity,
) -> Result<Option<String>> {
    let path = layout.identity_pin_path(identity);
    match std::fs::read_to_string(&path) {
        Ok(raw) => {
            let trimmed = raw.trim();
            Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("failed to read pin: {}", path.display())),
    }
}
```

Re-export from `lib.rs`.

- [ ] **Step 4: Add CLI scoped pin parser tests**

Add CLI test:

```rust
#[test]
fn cli_parses_pin_identity_selector_flags() {
    let cli = Cli::try_parse_from([
        "crosspack",
        "pin",
        "demo@<2.0.0",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--profile",
        "default",
        "--source",
        "unknown",
    ])
    .expect("pin selector flags should parse");
    match cli.command {
        Commands::Pin { spec, target, profile, source } => {
            assert_eq!(spec, "demo@<2.0.0");
            assert_eq!(target.as_deref(), Some("x86_64-unknown-linux-gnu"));
            assert_eq!(profile.as_deref(), Some("default"));
            assert_eq!(source.as_deref(), Some("unknown"));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}
```

- [ ] **Step 5: Implement scoped pin flags and dispatch**

Update `Commands::Pin`:

```rust
Pin {
    spec: String,
    #[arg(long)]
    target: Option<String>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    source: Option<String>,
},
```

Update dispatch:

```rust
Commands::Pin { spec, target, profile, source } => {
    let (name, requirement) = parse_pin_spec(&spec)?;
    let prefix = default_user_prefix()?;
    let layout = PrefixLayout::new(prefix);
    layout.ensure_base_dirs()?;
    let selector = InstalledPackageSelector {
        package: name.clone(),
        target,
        profile,
        source_namespace: source,
    };
    let pin_path = if selector.target.is_some() || selector.profile.is_some() || selector.source_namespace.is_some() {
        let Some(state) = resolve_installed_selector_for_cli(&layout, &selector)? else {
            return Err(anyhow!("cannot pin '{}': installed identity not found", selector.package));
        };
        write_identity_pin(&layout, &state.identity, &requirement.to_string())?
    } else {
        write_pin(&layout, &name, &requirement.to_string())?
    };
    for line in format_pin_status_lines(current_output_style(), &name, &requirement, &pin_path) {
        println!("{line}");
    }
}
```

- [ ] **Step 6: Update resolver pin collection**

Where `read_all_pins(layout)?` is collected in `core_flows.rs`, add a helper that overlays identity-scoped pins for selected installed states. Use legacy name pins as defaults and identity pins as more specific overrides.

```rust
fn collect_effective_pins_for_states(
    layout: &PrefixLayout,
    states: &[InstalledPackageState],
) -> Result<BTreeMap<String, VersionReq>> {
    let mut pins = BTreeMap::new();
    for (name, raw_req) in read_all_pins(layout)? {
        pins.insert(
            name.clone(),
            VersionReq::parse(&raw_req)
                .with_context(|| format!("invalid pin requirement for '{name}' in state: {raw_req}"))?,
        );
    }
    for state in states {
        if let Some(raw_req) = read_identity_pin(layout, &state.identity)? {
            pins.insert(
                state.receipt.name.clone(),
                VersionReq::parse(&raw_req).with_context(|| {
                    format!(
                        "invalid identity pin requirement for '{}': {raw_req}",
                        state.identity.selector_display()
                    )
                })?,
            );
        }
    }
    Ok(pins)
}
```

- [ ] **Step 7: Run pin tests**

Run:

```bash
cargo test -p crosspack-installer identity_scoped_pin_round_trip_does_not_replace_legacy_pin
cargo test -p crosspack-cli cli_parses_pin_identity_selector_flags parse_pin_spec_requires_constraint select_manifest_with_pin_applies_both_constraints
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/crosspack-installer/src/pins.rs crates/crosspack-installer/src/layout.rs crates/crosspack-installer/src/lib.rs crates/crosspack-installer/src/tests.rs crates/crosspack-cli/src/main.rs crates/crosspack-cli/src/dispatch.rs crates/crosspack-cli/src/core_flows.rs crates/crosspack-cli/src/tests.rs
git commit -m "feat(installer): support identity-scoped pins"
```

---

### Task 8: Prevent Cross-Identity Upgrade Grouping Regressions

**Files:**
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing upgrade grouping test**

Add near `build_upgrade_plans_groups_roots_by_target`:

```rust
#[test]
fn build_upgrade_plans_keeps_same_name_roots_separate_by_target() {
    let mut linux = InstallReceipt {
        name: "demo".to_string(),
        version: "1.0.0".to_string(),
        dependencies: Vec::new(),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        artifact_url: None,
        artifact_sha256: None,
        cache_path: None,
        exposed_bins: Vec::new(),
        exposed_completions: Vec::new(),
        snapshot_id: None,
        install_mode: InstallMode::Managed,
        install_reason: InstallReason::Root,
        install_status: "installed".to_string(),
        installed_at_unix: 1,
    };
    let mut macos = linux.clone();
    macos.target = Some("aarch64-apple-darwin".to_string());

    let plans = build_upgrade_plans(&[linux, macos]);
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].roots, vec![RootInstallRequest {
        name: "demo".to_string(),
        requirement: VersionReq::STAR,
    }]);
    assert_ne!(plans[0].target, plans[1].target);
}
```

If this already passes, keep it as regression coverage.

- [ ] **Step 2: Run test**

Run:

```bash
cargo test -p crosspack-cli build_upgrade_plans_keeps_same_name_roots_separate_by_target
```

Expected: PASS if target grouping is already correct, FAIL if current grouping collapses same-name roots.

- [ ] **Step 3: Thread selected identity into single upgrade**

Add selector flags to `Commands::Upgrade` only for single-package upgrade:

```rust
#[arg(long)]
profile: Option<String>,
#[arg(long)]
source: Option<String>,
```

For `--target`, reuse existing target behavior carefully: when a compact selector or installed selector target is provided for a single upgrade, it identifies the installed package. Do not confuse it with the resolved install target. If ambiguity is likely, prefer adding separate `--installed-target` over overloading `--target`.

Recommended minimal path for this implementation: do not add upgrade selector flags yet. Instead, use the installed identity resolver for bare-name ambiguity and keep global upgrade grouped by receipt target.

- [ ] **Step 4: Add explicit ambiguity test for single upgrade**

Add:

```rust
#[test]
fn upgrade_named_blocks_ambiguous_installed_package_name() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
        let mut receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
        receipt.target = Some(target.to_string());
        let state = InstalledPackageState {
            identity: InstalledPackageIdentity::from_legacy_receipt(&receipt),
            version: receipt.version.clone(),
            receipt,
            gui_assets: Vec::new(),
            native_gui_records: Vec::new(),
            services: Vec::new(),
            integrations: Vec::new(),
        };
        write_installed_package_state(&layout, &state).expect("must write state");
    }

    let err = run_upgrade_command(
        &layout,
        None,
        Some("demo".to_string()),
        UpgradeCommandOptions {
            target: None,
            dry_run: true,
            force_redownload: false,
            explain: false,
            build_from_source: false,
            provider_overrides: &[],
        },
    )
    .expect_err("ambiguous upgrade should fail");
    assert!(err.to_string().contains("installed package name 'demo' is ambiguous"));

    let _ = std::fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 5: Run upgrade tests**

Run:

```bash
cargo test -p crosspack-cli build_upgrade_plans_keeps_same_name_roots_separate_by_target upgrade_named_blocks_ambiguous_installed_package_name upgrade_all_transaction_preview_dry_run_output_matches_lifecycle_contract
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/crosspack-cli/src/core_flows.rs crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/tests.rs
git commit -m "fix(cli): keep upgrades identity-aware"
```

---

### Task 9: Add Doctor Diagnostics for Identity State Conflicts

**Files:**
- Modify: `crates/crosspack-installer/src/installed_state.rs`
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Test: `crates/crosspack-installer/src/tests.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing duplicate identity test**

Add installer test:

```rust
#[test]
fn read_all_installed_package_states_rejects_duplicate_identity_documents() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
    let state = InstalledPackageState {
        identity: InstalledPackageIdentity::from_legacy_receipt(&receipt),
        version: receipt.version.clone(),
        receipt,
        gui_assets: Vec::new(),
        native_gui_records: Vec::new(),
        services: Vec::new(),
        integrations: Vec::new(),
    };
    let path = write_installed_package_state(&layout, &state).expect("must write state");
    fs::copy(&path, layout.installed_state_document_path("demo-copy"))
        .expect("must duplicate state");

    let err = read_all_installed_package_states(&layout)
        .expect_err("duplicate identity must fail closed");
    assert!(err.to_string().contains("duplicate installed identity"));

    let _ = fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run failing test**

Run:

```bash
cargo test -p crosspack-installer read_all_installed_package_states_rejects_duplicate_identity_documents
```

Expected: FAIL because duplicates are currently deduplicated silently by `state_keys.insert()`.

- [ ] **Step 3: Fail closed on duplicate identity keys**

Update `read_all_installed_package_states()` so duplicate keys return an error:

```rust
if !state_keys.insert(state.identity.state_key()) {
    return Err(anyhow::anyhow!(
        "duplicate installed identity: {}",
        state.identity.state_key()
    ));
}
```

Apply this to both receipt-hydrated and document-discovered states. If legacy compatibility creates expected duplicate reads for the same file, skip the exact same path, not the same identity key from different files.

- [ ] **Step 4: Add doctor diagnostic test**

Find existing doctor transaction health tests and add a test that causes duplicate identity state, then verifies `doctor` reports a repair-style line. Expected line shape:

```text
installed_state: error duplicate-installed-identity
```

Do not change existing `transaction:` lines.

- [ ] **Step 5: Implement doctor line**

In `run_doctor_command` or the existing doctor formatting helper, call `read_all_installed_package_states(layout)` and map duplicate identity errors to:

```rust
"installed_state: error duplicate-installed-identity".to_string()
```

For clean state, emit either no line or:

```rust
"installed_state: clean".to_string()
```

Prefer no new line if existing doctor output contract would be noisy; if adding a line, update tests for the full doctor output.

- [ ] **Step 6: Run doctor and installer tests**

Run:

```bash
cargo test -p crosspack-installer read_all_installed_package_states_rejects_duplicate_identity_documents
cargo test -p crosspack-cli doctor installed_state
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/crosspack-installer/src/installed_state.rs crates/crosspack-installer/src/tests.rs crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/tests.rs
git commit -m "fix(installer): fail closed on duplicate installed identities"
```

---

### Task 10: Fail Closed for Ambiguous Service and Introspection Commands

**Files:**
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing ambiguity tests**

Add tests near `service_commands_require_declared_service_presence` and the dependency-introspection tests:

```rust
#[test]
fn service_start_blocks_ambiguous_installed_package_name() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
        let mut receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &[]);
        receipt.target = Some(target.to_string());
        let state = InstalledPackageState {
            identity: InstalledPackageIdentity::from_legacy_receipt(&receipt),
            version: receipt.version.clone(),
            receipt,
            gui_assets: Vec::new(),
            native_gui_records: Vec::new(),
            services: vec![ServiceDeclaration {
                name: "demo".to_string(),
                native_id: Some("demo.service".to_string()),
            }],
            integrations: Vec::new(),
        };
        write_installed_package_state(&layout, &state).expect("must write state");
    }

    let err = run_service_start_command(&layout, "demo")
        .expect_err("ambiguous service package should fail closed");
    assert!(err.to_string().contains("installed package name 'demo' is ambiguous"));

    let _ = std::fs::remove_dir_all(layout.prefix());
}

#[test]
fn depends_blocks_ambiguous_installed_package_name() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
        let mut receipt = install_receipt("demo", "1.0.0", InstallReason::Root, &["shared@1.0.0"]);
        receipt.target = Some(target.to_string());
        let state = InstalledPackageState {
            identity: InstalledPackageIdentity::from_legacy_receipt(&receipt),
            version: receipt.version.clone(),
            receipt,
            gui_assets: Vec::new(),
            native_gui_records: Vec::new(),
            services: Vec::new(),
            integrations: Vec::new(),
        };
        write_installed_package_state(&layout, &state).expect("must write state");
    }

    let err = run_depends_command(&layout, "demo")
        .expect_err("ambiguous depends target should fail closed");
    assert!(err.to_string().contains("installed package name 'demo' is ambiguous"));

    let _ = std::fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p crosspack-cli service_start_blocks_ambiguous_installed_package_name depends_blocks_ambiguous_installed_package_name
```

Expected: FAIL because service and depends commands currently use name-keyed state.

- [ ] **Step 3: Add a shared installed-name guard**

Add this helper near `parse_receipt_dependency_name()`:

```rust
fn ensure_installed_name_unambiguous(layout: &PrefixLayout, name: &str) -> Result<()> {
    let selector = InstalledPackageSelector {
        package: name.to_string(),
        target: None,
        profile: None,
        source_namespace: None,
    };
    resolve_installed_selector_for_cli(layout, &selector).map(|_| ())
}
```

Call it at the start of these functions before reading name-keyed receipts/state:

```rust
fn run_depends_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    let receipts = read_install_receipts(layout)?;
    // existing code continues
}

fn run_why_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    let receipts = read_install_receipts(layout)?;
    // existing code continues
}

fn run_service_status_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    // existing code continues
}

fn run_service_start_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    // existing code continues
}

fn run_service_stop_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    // existing code continues
}

fn run_service_restart_command(layout: &PrefixLayout, name: &str) -> Result<()> {
    ensure_installed_name_unambiguous(layout, name)?;
    // existing code continues
}
```

`run_uses_command(layout, name)` can remain name-oriented for this slice because it asks "who depends on this package name?" rather than selecting one installed identity for mutation. Add a dedicated test in the future only if product semantics change.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p crosspack-cli service_start_blocks_ambiguous_installed_package_name depends_blocks_ambiguous_installed_package_name service_commands_require_declared_service_presence
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/tests.rs
git commit -m "fix(cli): block ambiguous identity lifecycle reads"
```

---

### Task 11: Update Shipped Docs After Behavior Is Implemented

**Files:**
- Modify: `docs/architecture.md`
- Modify: `docs/install-flow.md`
- Modify: `.agents/specs/installed-identity-profile-model-spec.md`

- [ ] **Step 1: Update `docs/architecture.md`**

Add a short shipped-behavior note under the installed state/lifecycle section:

```markdown
Installed package state and new install storage are identity-aware. New installs write receipts, package payloads, sidecars, pins, and installed-state documents under identity-keyed paths that include profile, target, source namespace, and package fields. Source provenance is recorded separately for diagnostics. Legacy name-keyed receipts, sidecars, and package payloads remain readable and removable for compatibility. Lifecycle commands reject ambiguous bare package names before mutation and print deterministic selector guidance. `crosspack list --identity` exposes identity fields for automation; default `crosspack list` output remains `name version`.
```

- [ ] **Step 2: Update `docs/install-flow.md`**

Add lifecycle selector rules:

```markdown
Lifecycle commands resolve installed package selectors before mutation. A bare package name succeeds only when it matches exactly one installed identity. Ambiguous names fail before transaction start and print selector guidance using target/profile/source namespace fields. Identity-keyed installs use `pkgs/identities/v1/<profile>/<target>/<namespace>/<package>/<version>/` and identity-keyed receipt/sidecar paths. Legacy receipts hydrate as `profile=default`, `source_namespace=default`, and `source_provenance=unknown`.
```

- [ ] **Step 3: Update spec status note**

In `.agents/specs/installed-identity-profile-model-spec.md`, add a short implementation note under `## Current State` after the feature lands:

```markdown
Implementation note: identity-keyed package storage, identity receipt fields, selector parsing, installer-owned selector resolution, identity-aware list output, identity-scoped pins, identity-scoped uninstall/rollback routing, and duplicate identity diagnostics are implemented. Remaining future work should be tracked in follow-up specs or plans rather than this initial implementation plan.
```

- [ ] **Step 4: Run docs whitespace check**

Run:

```bash
git diff --check -- docs/architecture.md docs/install-flow.md .agents/specs/installed-identity-profile-model-spec.md
```

Expected: no output.

- [ ] **Step 5: Commit**

```bash
git add docs/architecture.md docs/install-flow.md .agents/specs/installed-identity-profile-model-spec.md
git commit -m "docs: document installed identity selectors"
```

---

### Task 12: Full Verification Gate

**Files:**
- No source edits unless verification fails.

- [ ] **Step 1: Run formatting**

```bash
cargo fmt --all --check
```

Expected: exit 0.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: exit 0.

- [ ] **Step 3: Run build**

```bash
cargo build --workspace --locked
```

Expected: exit 0.

- [ ] **Step 4: Run tests**

```bash
cargo test --workspace
```

Expected: exit 0.

- [ ] **Step 5: Run snapshot flow validation**

Because this plan touches lifecycle selection and transaction-adjacent mutation routing, run:

```bash
scripts/validate-snapshot-flow.sh
```

Expected:

```text
result: PASS - snapshot flow validation is healthy.
```

- [ ] **Step 6: Review final diff**

```bash
git status --short
git diff --stat origin/main...HEAD
```

Expected: only installer/CLI/resolver/docs/spec files intentionally touched by this plan.

---

## Self-Review Notes

- Spec coverage: tasks cover identity-keyed storage, selectors, ambiguity, legacy hydration, identity list output, identity-scoped pins, identity-scoped uninstall, upgrade grouping, duplicate identity repair diagnostics, rollback-adjacent verification, and docs updates.
- Compatibility boundary: legacy name-keyed receipt, sidecar, and payload paths remain readable/removable. New installs use identity-keyed paths so concurrent same-name installs can coexist.
- Output contracts: default `crosspack list` remains `name version`; dry-run transaction preview tokens are not changed.
- Risk: overloading `upgrade --target` could confuse installed selector target with requested install target. This plan avoids that by not adding upgrade selector flags until a separate command-surface decision is made.
