# CROSSPACK-INSTALLER KNOWLEDGE

## OVERVIEW
Install/uninstall lifecycle crate; receipt state, extraction staging, dependency-prune traversal all in `crates/crosspack-installer/src/lib.rs`.

## WHERE TO LOOK
- Receipt write format: `write_install_receipt` in `crates/crosspack-installer/src/lib.rs:214`.
- Receipt parse contract + backward-compat defaults: `parse_receipt` in `crates/crosspack-installer/src/lib.rs:486`.
- Receipt load pipeline (`.receipt` filtering, parse error context): `read_install_receipts` in `crates/crosspack-installer/src/lib.rs:252`.
- Uninstall orchestrator (block/prune decisions): `uninstall_package` in `crates/crosspack-installer/src/lib.rs:363`.
- Uninstall graph walk helpers: `dependency_map`, `reachable_packages`, `package_reachable` in `crates/crosspack-installer/src/lib.rs:566`, `crates/crosspack-installer/src/lib.rs:586`, `crates/crosspack-installer/src/lib.rs:603`.
- Artifact/receipt/bin deletion unit: `remove_receipt_artifacts` in `crates/crosspack-installer/src/lib.rs:536`.
- Cache prune safety gate: `safe_cache_prune_path` in `crates/crosspack-installer/src/lib.rs:466`.
- Install staging entrypoint: `install_from_artifact` in `crates/crosspack-installer/src/lib.rs:170`.
- Archive extraction dispatch: `extract_archive`, `extract_tar`, `extract_zip` in `crates/crosspack-installer/src/lib.rs:722`, `crates/crosspack-installer/src/lib.rs:729`, `crates/crosspack-installer/src/lib.rs:740`.
- Strip-components path rewrite: `copy_with_strip`, `copy_with_strip_recursive`, `strip_rel_components` in `crates/crosspack-installer/src/lib.rs:852`, `crates/crosspack-installer/src/lib.rs:870`, `crates/crosspack-installer/src/lib.rs:932`.
- Path layout contract used by install/uninstall: `PrefixLayout` in `crates/crosspack-installer/src/lib.rs:13`.

## CONVENTIONS
- Receipt file is line-based `k=v`; repeated keys allowed for lists (`dependency=`, `exposed_bin=`).
- `install_reason` parses via `InstallReason::parse`; unknown value is hard error.
- Missing `install_reason` defaults to `Root`; missing `install_status` defaults to `"installed"`; missing `installed_at_unix` is hard error.
- Receipt enumeration accepts only `*.receipt` regular files from `PrefixLayout::installed_state_dir()`.
- Uninstall dependency edges come from receipt `dependencies` entries parsed as `name@version`; invalid/missing `@` edge dropped.
- Root retention rule: package removal blocked when target remains reachable from other root receipts.
- Prune rule: target closure minus reachable-from-remaining-roots becomes `pruned_dependencies`.
- Cache prune allowed only for absolute paths under `PrefixLayout::artifacts_cache_dir()` and without `..` components.
- Install extraction uses tmp dirs under `PrefixLayout::tmp_state_dir()` with `raw/` then `staged/` subdirs.
- `artifact_root` is existence-checked under extracted `raw/`; strip behavior still governed by `strip_components` copy pass.
- Archive extraction order for zip: PowerShell (Windows) -> `unzip` -> `tar` fallback; tar variants use `tar -xf`.
- Binary exposure requires relative non-empty path without parent traversal (`validated_relative_binary_path`).

## ANTI-PATTERNS
- Changing receipt key names/order without updating both `write_install_receipt` and `parse_receipt` + compatibility tests.
- Parsing uninstall dependencies from package dirs instead of receipt state (`read_install_receipts` is source of truth).
- Deleting cache files directly from `cache_path` without `safe_cache_prune_path` gate.
- Treating malformed receipt lines as fatal globally; parser intentionally ignores unknown keys and malformed non-`=` lines.
- Removing target package before blocked-by-dependents reachability check.
- Using string concatenation for lifecycle paths instead of `PrefixLayout` helpers.
- Skipping `copy_with_strip`/`strip_rel_components` invariants when modifying extraction path logic.
- Replacing command fallback chain in `extract_zip` with a single-tool assumption.
- Dropping stale-state branch (`RepairedStaleState`) in uninstall behavior.
- Editing lifecycle logic without running targeted tests in `crates/crosspack-installer/src/lib.rs` test module.
