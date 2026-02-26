
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn source_store_add_rejects_duplicate_name() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    store
        .add_source(source_record("official", 10))
        .expect("must add source");
    let err = store
        .add_source(source_record("official", 5))
        .expect_err("must reject duplicate source name");
    assert!(err.to_string().contains("already exists"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_add_rejects_invalid_name() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    let err = store
        .add_source(source_record("bad name", 10))
        .expect_err("must reject invalid source name");
    assert!(err.to_string().contains("invalid source name"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_add_rejects_name_with_leading_separator() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    let err = store
        .add_source(source_record("-bad", 10))
        .expect_err("must reject invalid source name");
    assert!(err.to_string().contains("invalid source name"));

    let err = store
        .add_source(source_record("_bad", 10))
        .expect_err("must reject invalid source name");
    assert!(err.to_string().contains("invalid source name"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_add_rejects_name_longer_than_sixty_four_characters() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    let too_long_name = "a".repeat(65);
    let err = store
        .add_source(source_record(&too_long_name, 10))
        .expect_err("must reject invalid source name");
    assert!(err.to_string().contains("invalid source name"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_add_rejects_invalid_fingerprint() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    let mut record = source_record("official", 10);
    record.fingerprint_sha256 = "xyz".to_string();
    let err = store
        .add_source(record)
        .expect_err("must reject invalid fingerprint");
    assert!(err.to_string().contains("invalid source fingerprint"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_list_sorts_by_priority_then_name() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    store
        .add_source(source_record("zeta", 10))
        .expect("must add source");
    store
        .add_source(source_record("alpha", 1))
        .expect("must add source");
    store
        .add_source(source_record("beta", 10))
        .expect("must add source");

    let listed = store.list_sources().expect("must list sources");
    let names: Vec<&str> = listed.iter().map(|record| record.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta", "zeta"]);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_list_sources_with_snapshot_state_returns_none_when_snapshot_missing() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);
    store
        .add_source(source_record("official", 1))
        .expect("must add source");

    let listed = store
        .list_sources_with_snapshot_state()
        .expect("must list source states");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source.name, "official");
    assert_eq!(listed[0].snapshot, RegistrySourceSnapshotState::None);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_list_sources_with_snapshot_state_returns_ready_snapshot() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);
    store
        .add_source(source_record("official", 1))
        .expect("must add source");

    let cache_root = root.join("cache").join("official");
    fs::create_dir_all(&cache_root).expect("must create cache root");
    fs::write(
        cache_root.join("snapshot.json"),
        r#"{
  "version": 1,
  "source": "official",
  "snapshot_id": "git:0123456789abcdef",
  "updated_at_unix": 1,
  "manifest_count": 0,
  "status": "ready"
}"#,
    )
    .expect("must write snapshot file");

    let listed = store
        .list_sources_with_snapshot_state()
        .expect("must list source states");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].snapshot,
        RegistrySourceSnapshotState::Ready {
            snapshot_id: "git:0123456789abcdef".to_string()
        }
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_list_sources_with_snapshot_state_maps_invalid_snapshot_to_error_reason_code() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);
    store
        .add_source(source_record("official", 1))
        .expect("must add source");

    let cache_root = root.join("cache").join("official");
    fs::create_dir_all(&cache_root).expect("must create cache root");
    fs::write(cache_root.join("snapshot.json"), "{not-json").expect("must write invalid snapshot");

    let listed = store
        .list_sources_with_snapshot_state()
        .expect("must list source states");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].snapshot,
        RegistrySourceSnapshotState::Error {
            status: RegistrySourceWithSnapshotStatus::Unreadable,
            reason_code: "snapshot-unreadable".to_string(),
        }
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_remove_reports_missing_source() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    let err = store
        .remove_source("missing")
        .expect_err("must report missing source");
    assert!(err.to_string().contains("not found"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_remove_with_cache_purge_removes_source_cache_directory() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);
    store
        .add_source(source_record("official", 1))
        .expect("must add source");

    let cache_path = root.join("cache").join("official");
    fs::create_dir_all(&cache_path).expect("must create cache path");
    fs::write(cache_path.join("snapshot.json"), "{}").expect("must write cache fixture file");

    store
        .remove_source_with_cache_purge("official", true)
        .expect("must remove source and cache");

    assert!(!cache_path.exists(), "cache path must be removed");
    assert!(
        store
            .list_sources()
            .expect("must list sources")
            .into_iter()
            .all(|source| source.name != "official"),
        "source must be removed from source state"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_remove_without_cache_purge_keeps_source_cache_directory() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);
    store
        .add_source(source_record("official", 1))
        .expect("must add source");

    let cache_path = root.join("cache").join("official");
    fs::create_dir_all(&cache_path).expect("must create cache path");
    fs::write(cache_path.join("snapshot.json"), "{}").expect("must write cache fixture file");

    store
        .remove_source_with_cache_purge("official", false)
        .expect("must remove source and keep cache");

    assert!(
        cache_path.exists(),
        "cache path must remain when purge disabled"
    );
    assert!(
        store
            .list_sources()
            .expect("must list sources")
            .into_iter()
            .all(|source| source.name != "official"),
        "source must be removed from source state"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_writes_versioned_sorted_state_file() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    store
        .add_source(source_record("zeta", 10))
        .expect("must add source");
    store
        .add_source(source_record("alpha", 0))
        .expect("must add source");
    store
        .add_source(source_record("beta", 10))
        .expect("must add source");

    let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
    let expected = concat!(
            "version = 1\n",
            "\n",
            "[[sources]]\n",
            "name = \"alpha\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/alpha.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 0\n",
            "\n",
            "[[sources]]\n",
            "name = \"beta\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/beta.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 10\n",
            "\n",
            "[[sources]]\n",
            "name = \"zeta\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/zeta.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 10\n"
        );
    assert_eq!(content, expected);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_loads_unversioned_state_file_for_back_compat() {
    let root = test_registry_root();
    fs::create_dir_all(&root).expect("must create state root");
    fs::write(
            root.join("sources.toml"),
            concat!(
                "[[sources]]\n",
                "name = \"zeta\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/zeta.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 10\n",
                "\n",
                "[[sources]]\n",
                "name = \"alpha\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/alpha.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 0\n"
            ),
        )
        .expect("must write legacy state file");

    let store = RegistrySourceStore::new(&root);
    let listed = store.list_sources().expect("must list sources");
    let names: Vec<&str> = listed.iter().map(|record| record.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "zeta"]);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_rejects_negative_priority_in_legacy_state() {
    let root = test_registry_root();
    fs::create_dir_all(&root).expect("must create state root");
    fs::write(
            root.join("sources.toml"),
            concat!(
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = -1\n"
            ),
        )
        .expect("must write legacy state file");

    let store = RegistrySourceStore::new(&root);
    let err = store
        .list_sources()
        .expect_err("must reject negative source priority");
    assert!(err.to_string().contains("failed parsing source state"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_rejects_duplicate_names_in_loaded_state() {
    let root = test_registry_root();
    fs::create_dir_all(&root).expect("must create state root");
    fs::write(
            root.join("sources.toml"),
            concat!(
                "version = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official-mirror.git\"\n",
                "fingerprint_sha256 = \"fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210\"\n",
                "priority = 2\n"
            ),
        )
        .expect("must write duplicate source state file");

    let store = RegistrySourceStore::new(&root);
    let err = store
        .list_sources()
        .expect_err("must reject duplicate source names from disk");
    let rendered = format!("{err:#}");
    assert!(rendered.contains("duplicate source name 'official'"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_rejects_invalid_fingerprint_in_loaded_state() {
    let root = test_registry_root();
    fs::create_dir_all(&root).expect("must create state root");
    fs::write(
        root.join("sources.toml"),
        concat!(
            "version = 1\n",
            "\n",
            "[[sources]]\n",
            "name = \"official\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/official.git\"\n",
            "fingerprint_sha256 = \"xyz\"\n",
            "priority = 1\n"
        ),
    )
    .expect("must write invalid fingerprint state file");

    let store = RegistrySourceStore::new(&root);
    let err = store
        .list_sources()
        .expect_err("must reject invalid source fingerprint from disk");
    let rendered = format!("{err:#}");
    assert!(rendered.contains("invalid source fingerprint"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_rejects_unknown_state_file_version() {
    let root = test_registry_root();
    fs::create_dir_all(&root).expect("must create state root");
    fs::write(
            root.join("sources.toml"),
            concat!(
                "version = 7\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 1\n"
            ),
        )
        .expect("must write unknown version state file");

    let store = RegistrySourceStore::new(&root);
    let err = store
        .list_sources()
        .expect_err("must reject unknown source state schema version");
    let rendered = format!("{err:#}");
    assert!(
        rendered.contains("unsupported source state version"),
        "expected explicit unsupported version error, got: {err}"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_defaults_enabled_to_true_when_missing_in_loaded_state() {
    let root = test_registry_root();
    fs::create_dir_all(&root).expect("must create state root");
    fs::write(
            root.join("sources.toml"),
            concat!(
                "version = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 1\n"
            ),
        )
        .expect("must write state file without enabled flag");

    let store = RegistrySourceStore::new(&root);
    store
        .add_source(source_record("mirror", 2))
        .expect("must add source");

    let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
    assert!(
            content.contains(
                "name = \"official\"\nkind = \"git\"\nlocation = \"https://example.com/official.git\"\nfingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nenabled = true\npriority = 1\n"
            ),
            "expected missing enabled flag to default to true when persisted\n{content}"
        );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn source_store_first_write_from_empty_uses_version_one() {
    let root = test_registry_root();
    let store = RegistrySourceStore::new(&root);

    store
        .add_source(source_record("official", 0))
        .expect("must add first source");

    let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
    assert!(
        content.starts_with("version = 1\n"),
        "expected first write to persist version 1\n{content}"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn update_filesystem_source_writes_ready_snapshot() {
    let root = test_registry_root();
    let source_root = filesystem_source_fixture();
    let store = RegistrySourceStore::new(&root);

    let registry_pub = fs::read(source_root.join("registry.pub")).expect("must read registry pub");
    store
        .add_source(filesystem_source_record(
            "local",
            source_root
                .to_str()
                .expect("filesystem source path must be valid UTF-8"),
            sha256_hex_bytes(&registry_pub),
            0,
        ))
        .expect("must add source");

    let results = store.update_sources(&[]).expect("must update source");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "local");
    assert_eq!(results[0].status, SourceUpdateStatus::Updated);

    let cache_root = root.join("cache").join("local");
    assert!(cache_root.join("registry.pub").exists());
    assert!(cache_root.join("index").exists());

    let snapshot_path = cache_root.join("snapshot.json");
    let snapshot_content = fs::read_to_string(&snapshot_path).expect("must write snapshot.json");
    let snapshot: serde_json::Value =
        serde_json::from_str(&snapshot_content).expect("must parse snapshot.json");
    assert_eq!(snapshot["source"], "local");
    assert_eq!(snapshot["status"], "ready");
    assert_eq!(snapshot["manifest_count"], 1);
    assert_eq!(snapshot["snapshot_id"], results[0].snapshot_id.as_str());

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn update_filesystem_source_fails_on_fingerprint_mismatch() {
    let root = test_registry_root();
    let source_root = filesystem_source_fixture();
    let store = RegistrySourceStore::new(&root);

    store
        .add_source(filesystem_source_record(
            "local",
            source_root
                .to_str()
                .expect("filesystem source path must be valid UTF-8"),
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            0,
        ))
        .expect("must add source");

    let results = store
        .update_sources(&[])
        .expect("update API must report per-source failure");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, SourceUpdateStatus::Failed);
    assert!(results[0]
        .error
        .as_deref()
        .expect("must include error message")
        .contains("source-key-fingerprint-mismatch"));
    assert!(!root.join("cache").join("local").exists());

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn update_filesystem_source_accepts_uppercase_configured_fingerprint() {
    let root = test_registry_root();
    let source_root = filesystem_source_fixture();
    let store = RegistrySourceStore::new(&root);

    let registry_pub = fs::read(source_root.join("registry.pub")).expect("must read registry pub");
    store
        .add_source(filesystem_source_record(
            "local",
            source_root
                .to_str()
                .expect("filesystem source path must be valid UTF-8"),
            sha256_hex_bytes(&registry_pub).to_uppercase(),
            0,
        ))
        .expect("must add source");

    let results = store.update_sources(&[]).expect("must update source");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, SourceUpdateStatus::Updated);

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn update_filesystem_source_preserves_existing_cache_on_failure() {
    let root = test_registry_root();
    let source_root = filesystem_source_fixture();
    let store = RegistrySourceStore::new(&root);

    let registry_pub = fs::read(source_root.join("registry.pub")).expect("must read registry pub");
    let fingerprint = sha256_hex_bytes(&registry_pub);
    store
        .add_source(filesystem_source_record(
            "local",
            source_root
                .to_str()
                .expect("filesystem source path must be valid UTF-8"),
            fingerprint,
            0,
        ))
        .expect("must add source");

    let first = store
        .update_sources(&[])
        .expect("must update source initially");
    assert_eq!(first[0].status, SourceUpdateStatus::Updated);

    fs::remove_file(
        source_root
            .join("index")
            .join("ripgrep")
            .join("14.1.0.toml.sig"),
    )
    .expect("must remove signature to force verification failure");

    let second = store
        .update_sources(&[])
        .expect("update API must report per-source failure");
    assert_eq!(second[0].status, SourceUpdateStatus::Failed);

    let cached_signature = root
        .join("cache")
        .join("local")
        .join("index")
        .join("ripgrep")
        .join("14.1.0.toml.sig");
    assert!(
        cached_signature.exists(),
        "must preserve previous verified cache on update failure"
    );

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn update_filesystem_source_reports_updated_when_manifest_changes_with_same_key() {
    let root = test_registry_root();
    let source_root = filesystem_source_fixture();
    let store = RegistrySourceStore::new(&root);

    let registry_pub = fs::read(source_root.join("registry.pub")).expect("must read registry pub");
    let fingerprint = sha256_hex_bytes(&registry_pub);
    store
        .add_source(filesystem_source_record(
            "local",
            source_root
                .to_str()
                .expect("filesystem source path must be valid UTF-8"),
            fingerprint,
            0,
        ))
        .expect("must add source");

    let first = store
        .update_sources(&[])
        .expect("first update must succeed");
    assert_eq!(first[0].status, SourceUpdateStatus::Updated);

    rewrite_signed_manifest_with_extra_field(
        &source_root
            .join("index")
            .join("ripgrep")
            .join("14.1.0.toml"),
        &signing_key(),
        "description = \"updated\"\n",
    );

    let second = store
        .update_sources(&[])
        .expect("second update must succeed");
    assert_eq!(second[0].status, SourceUpdateStatus::Updated);

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn update_git_source_clones_and_records_snapshot_id() {
    let root = test_registry_root();
    let source_root = git_source_fixture();
    let store = RegistrySourceStore::new(&root);

    let registry_pub = fs::read(source_root.join("registry.pub")).expect("must read registry pub");
    let source_location = git_fixture_location(&source_root);
    store
        .add_source(git_source_record(
            "origin",
            &source_location,
            sha256_hex_bytes(&registry_pub),
            0,
        ))
        .expect("must add source");

    let expected_snapshot_id = git_head_short(&source_root);
    let results = store.update_sources(&[]).expect("must update source");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, SourceUpdateStatus::Updated);
    assert_eq!(results[0].snapshot_id, expected_snapshot_id);

    let cache_root = root.join("cache").join("origin");
    assert!(cache_root.join("registry.pub").exists());
    assert!(cache_root.join("index").exists());
    assert_eq!(git_head_short(&cache_root), expected_snapshot_id);

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn update_git_source_fetches_new_commit() {
    let root = test_registry_root();
    let source_root = git_source_fixture();
    let store = RegistrySourceStore::new(&root);

    let registry_pub = fs::read(source_root.join("registry.pub")).expect("must read registry pub");
    let source_location = git_fixture_location(&source_root);
    store
        .add_source(git_source_record(
            "origin",
            &source_location,
            sha256_hex_bytes(&registry_pub),
            0,
        ))
        .expect("must add source");

    let first = store
        .update_sources(&[])
        .expect("first update must succeed");
    assert_eq!(first[0].status, SourceUpdateStatus::Updated);

    rewrite_signed_manifest_with_extra_field(
        &source_root
            .join("index")
            .join("ripgrep")
            .join("14.1.0.toml"),
        &signing_key(),
        "description = \"updated\"\n",
    );
    git_commit_all(&source_root, "update ripgrep manifest");

    let expected_snapshot_id = git_head_short(&source_root);
    let second = store
        .update_sources(&[])
        .expect("second update must succeed");
    assert_eq!(second[0].status, SourceUpdateStatus::Updated);
    assert_eq!(second[0].snapshot_id, expected_snapshot_id);

    let cached_manifest = fs::read_to_string(
        root.join("cache")
            .join("origin")
            .join("index")
            .join("ripgrep")
            .join("14.1.0.toml"),
    )
    .expect("must read cached manifest");
    assert!(cached_manifest.contains("description = \"updated\""));

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn update_git_source_returns_up_to_date_when_revision_unchanged() {
    let root = test_registry_root();
    let source_root = git_source_fixture();
    let store = RegistrySourceStore::new(&root);

    let registry_pub = fs::read(source_root.join("registry.pub")).expect("must read registry pub");
    let source_location = git_fixture_location(&source_root);
    store
        .add_source(git_source_record(
            "origin",
            &source_location,
            sha256_hex_bytes(&registry_pub),
            0,
        ))
        .expect("must add source");

    let first = store
        .update_sources(&[])
        .expect("first update must succeed");
    assert_eq!(first[0].status, SourceUpdateStatus::Updated);

    let second = store
        .update_sources(&[])
        .expect("second update must succeed");
    assert_eq!(second[0].status, SourceUpdateStatus::UpToDate);
    assert_eq!(second[0].snapshot_id, first[0].snapshot_id);

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn git_snapshot_id_derives_source_prefixed_fixed_width_prefix_from_full_sha() {
    let full_sha = "0123456789abcdef0123456789abcdef01234567";
    let snapshot_id =
        derive_snapshot_id_from_full_git_sha(full_sha).expect("must derive snapshot id");
    assert_eq!(snapshot_id, "git:0123456789abcdef");
    assert_eq!(snapshot_id.len(), 20);
}

#[test]
fn git_snapshot_id_derivation_rejects_short_sha() {
    let err = derive_snapshot_id_from_full_git_sha("0123456789abcde")
        .expect_err("must reject sha values shorter than sixteen characters");
    assert!(err.to_string().contains("too short"));
}

#[test]
fn rollback_replace_error_includes_restore_failure_context() {
    let replace_err = anyhow::anyhow!("replace failed");
    let restore_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");

    let err = combine_replace_restore_errors(
        "local",
        Path::new("/tmp/cache/local"),
        Path::new("/tmp/cache/.local-backup"),
        replace_err,
        restore_err,
    );

    let rendered = format!("{err:#}");
    assert!(rendered.contains("failed replacing cache"));
    assert!(rendered.contains("failed restoring backup"));
    assert!(rendered.contains("denied"));
}

#[test]
fn update_unknown_source_returns_source_not_found() {
    let root = test_registry_root();
    let source_root = filesystem_source_fixture();
    let store = RegistrySourceStore::new(&root);

    let registry_pub = fs::read(source_root.join("registry.pub")).expect("must read registry pub");
    store
        .add_source(filesystem_source_record(
            "local",
            source_root
                .to_str()
                .expect("filesystem source path must be valid UTF-8"),
            sha256_hex_bytes(&registry_pub),
            0,
        ))
        .expect("must add source");

    let err = store
        .update_sources(&[String::from("missing")])
        .expect_err("must reject unknown source names");
    assert!(err.to_string().contains("source-not-found"));

    let _ = fs::remove_dir_all(&source_root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn search_names_fails_when_registry_public_key_is_missing() {
    let root = test_registry_root();
    let package_dir = root.join("index").join("ripgrep");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(
        package_dir.join("14.1.0.toml"),
        manifest_toml("ripgrep", "14.1.0"),
    )
    .expect("must write manifest");

    let index = RegistryIndex::open(&root);
    let err = index
        .search_names("rip")
        .expect_err("must fail when registry.pub is missing");
    assert!(err.to_string().contains("registry.pub"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn configured_index_package_versions_prefers_higher_priority_source() {
    let state_root = test_registry_root();
    let store = RegistrySourceStore::new(&state_root);

    store
        .add_source(source_record("fallback", 10))
        .expect("must add fallback source");
    store
        .add_source(source_record("preferred", 0))
        .expect("must add preferred source");

    let fallback_key = SigningKey::from_bytes(&[11u8; 32]);
    let preferred_key = SigningKey::from_bytes(&[13u8; 32]);
    write_ready_snapshot_cache(&state_root, "fallback", &fallback_key, &["14.0.0"]);
    write_ready_snapshot_cache(&state_root, "preferred", &preferred_key, &["14.1.0"]);

    let index = ConfiguredRegistryIndex::open(&state_root).expect("must open configured index");
    let manifests = index
        .package_versions("ripgrep")
        .expect("must read package from highest precedence source");

    let versions: Vec<String> = manifests
        .iter()
        .map(|manifest| manifest.version.to_string())
        .collect();
    assert_eq!(versions, vec!["14.1.0"]);

    let _ = fs::remove_dir_all(&state_root);
}

#[test]
fn configured_index_package_versions_uses_name_tiebreaker() {
    let state_root = test_registry_root();
    let store = RegistrySourceStore::new(&state_root);

    store
        .add_source(source_record("beta", 1))
        .expect("must add beta source");
    store
        .add_source(source_record("alpha", 1))
        .expect("must add alpha source");

    let beta_key = SigningKey::from_bytes(&[17u8; 32]);
    let alpha_key = SigningKey::from_bytes(&[19u8; 32]);
    write_ready_snapshot_cache(&state_root, "beta", &beta_key, &["14.0.0"]);
    write_ready_snapshot_cache(&state_root, "alpha", &alpha_key, &["14.2.0"]);

    let index = ConfiguredRegistryIndex::open(&state_root).expect("must open configured index");
    let manifests = index
        .package_versions("ripgrep")
        .expect("must apply source-name tie-breaker");

    let versions: Vec<String> = manifests
        .iter()
        .map(|manifest| manifest.version.to_string())
        .collect();
    assert_eq!(versions, vec!["14.2.0"]);

    let _ = fs::remove_dir_all(&state_root);
}

#[test]
fn configured_index_search_names_deduplicates_across_sources() {
    let state_root = test_registry_root();
    let store = RegistrySourceStore::new(&state_root);

    store
        .add_source(source_record("one", 0))
        .expect("must add first source");
    store
        .add_source(source_record("two", 1))
        .expect("must add second source");

    let first_key = SigningKey::from_bytes(&[23u8; 32]);
    let second_key = SigningKey::from_bytes(&[29u8; 32]);
    write_ready_snapshot_cache(&state_root, "one", &first_key, &["14.1.0"]);
    write_ready_snapshot_cache(&state_root, "two", &second_key, &["14.0.0"]);

    let index = ConfiguredRegistryIndex::open(&state_root).expect("must open configured index");
    let names = index
        .search_names("rip")
        .expect("must deduplicate package names");
    assert_eq!(names, vec!["ripgrep"]);

    let _ = fs::remove_dir_all(&state_root);
}

#[test]
fn configured_index_fails_when_no_ready_snapshot_exists() {
    let state_root = test_registry_root();
    let store = RegistrySourceStore::new(&state_root);

    store
        .add_source(source_record("official", 0))
        .expect("must add source");

    let err = ConfiguredRegistryIndex::open(&state_root)
        .expect_err("must fail when no enabled source has a ready snapshot");
    assert!(err.to_string().contains("no ready snapshot"));

    let _ = fs::remove_dir_all(&state_root);
}

#[test]
fn configured_index_open_fails_when_sources_file_is_unreadable() {
    let state_root = test_registry_root();
    fs::create_dir_all(state_root.join("sources.toml")).expect("must make sources path unreadable");

    let err = ConfiguredRegistryIndex::open(&state_root)
        .expect_err("must fail when sources state cannot be read");
    assert!(err
        .to_string()
        .contains("failed reading configured registry sources"));

    let _ = fs::remove_dir_all(&state_root);
}

#[test]
fn search_names_returns_matching_package_with_valid_signed_manifests() {
    let root = test_registry_root();
    let package_dir = root.join("index").join("ripgrep");
    fs::create_dir_all(&package_dir).expect("must create package dir");

    let signing_key = signing_key();
    fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
        .expect("must write registry public key");
    write_signed_manifest(&package_dir, &signing_key, "14.1.0");

    let index = RegistryIndex::open(&root);
    let names = index
        .search_names("rip")
        .expect("must load matching package names");
    assert_eq!(names, vec!["ripgrep"]);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_versions_fails_when_registry_public_key_is_missing() {
    let root = test_registry_root();
    let package_dir = root.join("index").join("ripgrep");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    let manifest_path = package_dir.join("14.1.0.toml");
    fs::write(&manifest_path, manifest_toml("ripgrep", "14.1.0")).expect("must write manifest");

    let index = RegistryIndex::open(&root);
    let err = index
        .package_versions("ripgrep")
        .expect_err("must fail when registry.pub is missing");
    assert!(err.to_string().contains("registry.pub"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_versions_fails_when_manifest_signature_is_missing() {
    let root = test_registry_root();
    let package_dir = root.join("index").join("ripgrep");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    let manifest_path = package_dir.join("14.1.0.toml");
    fs::write(&manifest_path, manifest_toml("ripgrep", "14.1.0")).expect("must write manifest");

    let signing_key = signing_key();
    fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
        .expect("must write registry public key");

    let index = RegistryIndex::open(&root);
    let err = index
        .package_versions("ripgrep")
        .expect_err("must fail when signature sidecar is missing");
    assert!(err.to_string().contains(".sig"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_versions_fails_when_manifest_signature_is_invalid() {
    let root = test_registry_root();
    let package_dir = root.join("index").join("ripgrep");
    fs::create_dir_all(&package_dir).expect("must create package dir");

    let manifest = manifest_toml("ripgrep", "14.1.0");
    let manifest_path = package_dir.join("14.1.0.toml");
    fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

    let signing_key = signing_key();
    fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
        .expect("must write registry public key");

    fs::write(manifest_path.with_extension("toml.sig"), "00")
        .expect("must write invalid signature");

    let index = RegistryIndex::open(&root);
    let err = index
        .package_versions("ripgrep")
        .expect_err("must fail when signature is invalid");
    assert!(err.to_string().contains("signature"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_versions_succeeds_with_valid_signatures_and_descending_sort() {
    let root = test_registry_root();
    let package_dir = root.join("index").join("ripgrep");
    fs::create_dir_all(&package_dir).expect("must create package dir");

    let signing_key = signing_key();
    fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
        .expect("must write registry public key");

    write_signed_manifest(&package_dir, &signing_key, "14.0.0");
    write_signed_manifest(&package_dir, &signing_key, "14.1.0");

    let index = RegistryIndex::open(&root);
    let manifests = index
        .package_versions("ripgrep")
        .expect("must load manifests");

    let versions: Vec<String> = manifests
        .iter()
        .map(|manifest| manifest.version.to_string())
        .collect();
    assert_eq!(versions, vec!["14.1.0", "14.0.0"]);

    let _ = fs::remove_dir_all(&root);
}

fn write_signed_manifest(package_dir: &std::path::Path, signing_key: &SigningKey, version: &str) {
    let manifest_path = package_dir.join(format!("{version}.toml"));
    let manifest = manifest_toml("ripgrep", version);
    fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

    let signature = signing_key.sign(manifest.as_bytes());
    fs::write(
        manifest_path.with_extension("toml.sig"),
        hex::encode(signature.to_bytes()),
    )
    .expect("must write signature sidecar");
}

fn rewrite_signed_manifest_with_extra_field(
    manifest_path: &Path,
    signing_key: &SigningKey,
    extra_field: &str,
) {
    let mut manifest = fs::read_to_string(manifest_path).expect("must read manifest");
    manifest.push_str(extra_field);
    fs::write(manifest_path, manifest.as_bytes()).expect("must rewrite manifest");

    let signature = signing_key.sign(manifest.as_bytes());
    fs::write(
        manifest_path.with_extension("toml.sig"),
        hex::encode(signature.to_bytes()),
    )
    .expect("must rewrite signature sidecar");
}

fn manifest_toml(name: &str, version: &str) -> String {
    format!(
        r#"name = "{name}"
version = "{version}"
"#
    )
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn public_key_hex(signing_key: &SigningKey) -> String {
    hex::encode(signing_key.verifying_key().to_bytes())
}

fn source_record(name: &str, priority: u32) -> RegistrySourceRecord {
    RegistrySourceRecord {
        name: name.to_string(),
        kind: RegistrySourceKind::Git,
        location: format!("https://example.com/{name}.git"),
        fingerprint_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .to_string(),
        enabled: true,
        priority,
    }
}

fn git_source_record(
    name: &str,
    location: &str,
    fingerprint_sha256: String,
    priority: u32,
) -> RegistrySourceRecord {
    RegistrySourceRecord {
        name: name.to_string(),
        kind: RegistrySourceKind::Git,
        location: location.to_string(),
        fingerprint_sha256,
        enabled: true,
        priority,
    }
}

fn filesystem_source_record(
    name: &str,
    location: &str,
    fingerprint_sha256: String,
    priority: u32,
) -> RegistrySourceRecord {
    RegistrySourceRecord {
        name: name.to_string(),
        kind: RegistrySourceKind::Filesystem,
        location: location.to_string(),
        fingerprint_sha256,
        enabled: true,
        priority,
    }
}

fn filesystem_source_fixture() -> PathBuf {
    let root = test_registry_root();
    let package_dir = root.join("index").join("ripgrep");
    fs::create_dir_all(&package_dir).expect("must create package dir");

    let signing_key = signing_key();
    let public_key_hex = public_key_hex(&signing_key);
    fs::write(root.join("registry.pub"), public_key_hex.as_bytes())
        .expect("must write registry public key");
    write_signed_manifest(&package_dir, &signing_key, "14.1.0");

    root
}

fn git_source_fixture() -> PathBuf {
    let root = filesystem_source_fixture();
    git_run(&root, &["init"]);
    git_run(&root, &["add", "."]);
    git_commit_all(&root, "initial registry snapshot");
    root
}

fn git_fixture_location(path: &Path) -> String {
    #[cfg(windows)]
    {
        path.to_string_lossy().replace('\\', "/")
    }

    #[cfg(not(windows))]
    {
        path.to_string_lossy().to_string()
    }
}

fn git_head_short(repo_root: &Path) -> String {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(repo_root)
        .output()
        .expect("git must run rev-parse");
    assert!(
        output.status.success(),
        "git rev-parse failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let full_sha = String::from_utf8(output.stdout)
        .expect("git rev-parse output must be UTF-8")
        .trim()
        .to_string();
    derive_snapshot_id_from_full_git_sha(&full_sha)
        .expect("git rev-parse output must be a valid full sha")
}

fn git_commit_all(repo_root: &Path, message: &str) {
    git_run(repo_root, &["add", "."]);
    git_run(
        repo_root,
        &[
            "-c",
            "user.name=Crosspack Tests",
            "-c",
            "user.email=crosspack-tests@example.com",
            "commit",
            "-m",
            message,
        ],
    );
}

fn git_run(repo_root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .expect("git command must execute");
    assert!(
        output.status.success(),
        "git command failed: git {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sha256_hex_bytes(bytes: &[u8]) -> String {
    crosspack_security::sha256_hex(bytes)
}

fn write_ready_snapshot_cache(
    state_root: &Path,
    source_name: &str,
    signing_key: &SigningKey,
    versions: &[&str],
) {
    let cache_root = state_root.join("cache").join(source_name);
    let package_dir = cache_root.join("index").join("ripgrep");
    fs::create_dir_all(&package_dir).expect("must create package directory in cache");
    fs::write(cache_root.join("registry.pub"), public_key_hex(signing_key))
        .expect("must write registry key for cache source");

    for version in versions {
        write_signed_manifest(&package_dir, signing_key, version);
    }

    let snapshot = serde_json::json!({
        "version": 1,
        "source": source_name,
        "snapshot_id": format!("fs:{source_name}"),
        "updated_at_unix": 1,
        "manifest_count": versions.len(),
        "status": "ready"
    });
    fs::write(
        cache_root.join("snapshot.json"),
        serde_json::to_string_pretty(&snapshot).expect("must serialize snapshot"),
    )
    .expect("must write snapshot metadata");
}

static TEST_REGISTRY_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn test_registry_root() -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let counter = TEST_REGISTRY_ROOT_COUNTER.fetch_add(1, Ordering::SeqCst);
    path.push(format!(
        "crosspack-registry-tests-{}-{}-{}",
        std::process::id(),
        nanos,
        counter
    ));
    path
}
