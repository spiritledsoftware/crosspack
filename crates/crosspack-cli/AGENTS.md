# CROSSPACK-CLI KNOWLEDGE BASE

## OVERVIEW
Single-file CLI orchestrator: parses commands, coordinates registry/resolver/installer/security crates, emits stable user-facing lifecycle text.

## WHERE TO LOOK
| Task | File path | Hotspot |
|---|---|---|
| Add/rename command flags or subcommands | `crates/crosspack-cli/src/main.rs` | `Cli`, `Commands` enum (`Search/Info/Install/Upgrade/Uninstall/List/Pin/Doctor/InitShell`) |
| Change top-level command routing | `crates/crosspack-cli/src/main.rs` | `main()` `match cli.command` dispatch block |
| Install graph + target selection behavior | `crates/crosspack-cli/src/main.rs` | `resolve_install_graph`, `host_target_triple` |
| Install execution + receipt emission | `crates/crosspack-cli/src/main.rs` | `install_resolved`, `print_install_outcome` |
| Upgrade orchestration, root grouping, overlap safety | `crates/crosspack-cli/src/main.rs` | `build_upgrade_plans`, `enforce_disjoint_multi_target_upgrade`, `enforce_no_downgrades` |
| Pin parsing and policy | `crates/crosspack-cli/src/main.rs` | `parse_pin_spec` |
| Uninstall messaging contract | `crates/crosspack-cli/src/main.rs` | `format_uninstall_messages` |
| Binary exposure conflict checks | `crates/crosspack-cli/src/main.rs` | `collect_declared_binaries`, `validate_binary_name`, `validate_binary_preflight` |
| Artifact download path + fallback order | `crates/crosspack-cli/src/main.rs` | `download_artifact` -> `download_with_curl` -> `download_with_wget` / `download_with_powershell` -> `run_command` |
| CLI-specific dependency/version constraints | `crates/crosspack-cli/Cargo.toml` | clap/anyhow/semver pinning impacts parser + error text |
| Test focus: parser and pin constraints | `crates/crosspack-cli/src/main.rs` | `#[cfg(test)] mod tests`: `parse_pin_spec_*`, `select_manifest_with_pin_*` |
| Test focus: install ownership and binary collision guards | `crates/crosspack-cli/src/main.rs` | `#[cfg(test)] mod tests`: `validate_binary_preflight_*` |
| Test focus: upgrade plan construction + downgrade prevention | `crates/crosspack-cli/src/main.rs` | `#[cfg(test)] mod tests`: `build_upgrade_*`, `enforce_no_downgrades_*`, `enforce_disjoint_multi_target_upgrade_*` |
| Test focus: uninstall text stability | `crates/crosspack-cli/src/main.rs` | `#[cfg(test)] mod tests`: `format_uninstall_messages_*` |

## CONVENTIONS
- Keep orchestration in `crates/crosspack-cli/src/main.rs`; domain rules stay in library crates.
- New command arm must preserve deterministic stdout wording; message shape is part of UX contract.
- Prefer helper fns over enlarging `main()` branch bodies; command arms should read as flow, not implementation detail.
- Reuse `Result<()>` + `anyhow::Context` error style; include actionable context for user-facing failures.
- Preserve target-aware upgrade grouping by receipt `target`; do not collapse cross-target roots into one plan.
- Keep download writes atomic (`.part` then rename); remove partial files on error.
- Treat binary ownership as strict: reject collisions with other receipts and unmanaged files before install.
- Tests live in `crates/crosspack-cli/src/main.rs` under `#[cfg(test)]`; add/extend tests in same module when changing CLI behavior.

## ANTI-PATTERNS
- Adding a `Commands` variant without a matching `main()` arm and tests in `crates/crosspack-cli/src/main.rs`.
- Editing lifecycle strings (`installed`, `upgraded`, `uninstalled`, `up-to-date`, `not installed`) without updating affected tests.
- Bypassing `enforce_no_downgrades` in upgrade paths or silently allowing downgrade via upgrade flow.
- Skipping `enforce_disjoint_multi_target_upgrade` when touching multi-target upgrade orchestration.
- Writing command output from helper crates to "simplify" CLI code; output ownership stays in CLI crate.
- Replacing `download_artifact` fallback order with shell-specific assumptions that break cross-platform behavior.
- Accepting binary names with separators or mutating preflight checks to permit overwrite of foreign binaries.
