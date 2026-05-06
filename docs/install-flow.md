# Install Flow (current shipped behavior)

`crosspack install <name[@constraint]>` executes this sequence:

1. Select metadata backend, then verify registry metadata before resolution:
   - with `--registry-root`, read from that root directly (legacy single-root mode),
   - without `--registry-root`, read from configured snapshots under `<prefix>/state/registries/cache/`,
   - if no configured source has a ready snapshot, fail with guidance to run `crosspack registry add` and `crosspack update`,
   - trust `registry.pub` from the registry root,
   - require `<version>.toml.sig` detached sidecar for each manifest,
   - verify sidecar signatures from hex-encoded signature data.
2. Resolve package graph from registry manifests:
   - merge dependency constraints transitively,
   - apply pin constraints to root and transitive packages,
   - produce dependency-first install order.
3. Build typed `InstallPlan` data from the resolved graph and installed summaries:
   - planned adds, removals, replacements, and version transitions,
   - provider substitutions and conflict/replacement explainability evidence,
   - replacement root-intent preservation for packages that replace an installed root.
4. Select install plan for each resolved package for requested target (`--target` or host triple):
   - binary artifact path when target artifact is available,
   - source-build path when `--build-from-source` is set and validated `source_build` metadata is present.
5. Determine artifact kind (`artifact.archive` or infer from URL suffix): `zip`, `tar.gz`, `tar.zst`, `bin`, `msi`, `dmg`, `appimage`, `exe`, `pkg`, `msix`, `appx`.
   - Extensionless final URL path segments infer to `bin`.
   - Pre-1.0 scope reset: `deb` and `rpm` are removed from the supported artifact contract and are rejected.
6. For each resolved package, resolve cache path at:
   - `<prefix>/cache/artifacts/<name>/<version>/<target>/artifact.<ext>`
7. Download selected payload if needed (or if `--force-redownload`):
   - binary artifact URL for binary installs,
   - `source_build.url` for source installs.
   - backend selection env var: `CROSSPACK_DOWNLOAD_BACKEND` supports `in-process` (default) or `external`.
   - default (`in-process`) uses reqwest with bounded retry (up to 3 attempts) and falls back to external backend on failure.
   - `external` forces external downloader backend and skips in-process attempts.
   - external backend is cross-platform (`curl`/`wget` with Windows PowerShell support).
8. Verify SHA-256 before execution:
   - binary installs verify artifact bytes against manifest `sha256`,
   - source installs verify source archive bytes against `source_build.archive_sha256`.
9. Stage payload into temporary state directory with deterministic adapters:
   - managed mode adapters: `zip`, `tar.gz`, `tar.zst` (archive extraction), `bin` (copy payload using the cached file name; requires `strip_components=0` and no `artifact_root`), `dmg` (attach/copy/detach extraction on macOS), `appimage` (copy payload as `artifact.appimage` on Linux; requires `strip_components=0` and no `artifact_root`),
   - native mode defaults: `pkg` on macOS, `exe`/`msi`/`msix`/`appx` on Windows,
   - native mode still uses deterministic non-UI adapter execution; vendor installer fallback is not attempted.
10. Source-build path (when selected):
    - extract source archive,
    - run deterministic `build_commands`,
    - run deterministic `install_commands`,
    - install staged output from `CROSSPACK_STAGE_DIR` into the selected identity payload root.
11. Apply `strip_components` during staging copy where supported (binary artifact path).
12. Move staged content into `<prefix>/pkgs/identities/v1/<profile>/<target>/<namespace>/<name>/<version>/` for new installs; legacy payload roots remain readable for compatibility.
13. Preflight binary exposure collisions against existing receipts and on-disk `<prefix>/bin` entries.
14. Preflight package completion exposure collisions against existing receipts and on-disk completion files under `<prefix>/share/completions/packages/<shell>/`.
15. Apply replacement handoff from `InstallPlan` removals/replacements, failing before mutation if replaced packages are still required by remaining roots.
16. Expose declared binaries:
    - Unix: symlink `<prefix>/bin/<name>` to installed package path.
    - Windows: write `<prefix>/bin/<name>.cmd` shim to installed package path.
17. Expose declared package completion files to `<prefix>/share/completions/packages/<shell>/`.
18. Expose declared GUI application assets under `<prefix>/share/gui/` (launcher + handler metadata).
19. Register native GUI integrations as best-effort adapters; failures emit warning lines and do not fail successful install.
    - macOS `.app` registration uses bundle-copy deployment and tries `/Applications/<App>.app` before `~/Applications/<App>.app`.
    - Existing unmanaged app bundles at either macOS destination are not overwritten; registration emits warnings and continues.
20. Project typed Docker CLI, PATH plugin, and service integration payloads under `<prefix>/integrations/`.
21. Reject service `enable = true` before host mutation; Docker CLI, PATH plugin, and service host activation remain explicit.
22. Remove stale previously-owned binaries, completion files, GUI assets, native GUI registrations, and integration projections no longer declared for that package.
23. Persist declared manifest services to identity-keyed service state for service-command lookup.
24. Write install receipt to an identity-keyed receipt path.
      - persist `install_mode=managed|native` from artifact-kind defaults,
      - set `install_reason=root` for requested roots,
      - set `install_reason=dependency` for transitive-only packages,
      - preserve existing `install_reason=root` when upgrading already-rooted packages.
25. Write a versioned installed-state document keyed by installed identity while preserving legacy receipt/sidecar reads.
26. Best-effort refresh Crosspack shell completion assets under `<prefix>/share/completions/crosspack.<shell>` so package completion loaders are up to date.

`crosspack install --dry-run` executes the same planning and renders deterministic, script-friendly preview lines from typed `InstallPlan` data:
- `transaction_preview operation=... mode=dry-run`
- `transaction_summary adds=... removals=... replacements=... transitions=...`
- `risk_flags=...`
- ordered `change_*` entries (`change_add`, `change_remove`, `change_replace`, `change_transition`).
- no transaction metadata, receipts, package files, or binaries are mutated.

For non-dry-run lifecycle output, Crosspack auto-selects output mode:
- interactive terminal: enhanced terminal UX (section hierarchy, semantic color, progress indicators, and rich install detail rows),
- non-interactive/piped output: plain deterministic lines.

Interactive rich mode also renders live download telemetry during the download phase:
- when HTTP `Content-Length` is available, progress includes downloaded bytes and percent,
- when total size is unknown, progress includes downloaded bytes only.

In interactive mode, install detail rows are rendered in a dedicated rich shape:
- `STATUS | key: | value`
- no plain status badge tokens (`[OK]`, `[..]`, `[ERR]`, `[WARN]`) are used in install outcome detail rows.

Plain mode keeps existing deterministic line contracts unchanged (no live byte/percent progress frames).

Machine-oriented dry-run preview lines remain unchanged regardless of output mode.
Interactive enhancements are additive-only and must not change plain-mode semantics.

## Interaction and Escalation Policy Flags

Mutating commands (`install`, `upgrade`, `uninstall`, `rollback`, `repair`, `self-update`) share escalation policy flags:

- default interactive behavior (no flags): prompt and non-prompt escalation paths are both allowed,
- `--non-interactive`: prompt escalation is disabled; non-prompt escalation is also disabled unless `--allow-escalation` is set,
- `--non-interactive --allow-escalation`: only non-prompt escalation paths are allowed,
- `--no-escalation`: disables all escalation paths and overrides the interactive default,
- `--allow-escalation` conflicts with `--no-escalation`.

`upgrade` with no package argument runs one dependency solve per target group derived from installed root receipts.
`crosspack upgrade --dry-run` emits the same preview format and performs planning without mutation.

Lifecycle commands resolve installed package selectors before mutation. A bare package name succeeds only when it matches exactly one installed identity. Ambiguous names fail before transaction start and print selector guidance using target/profile/source namespace fields. Legacy receipts hydrate as `profile=default`, `source_namespace=default`, and `source_provenance=unknown`.

## Transaction Phases and Recovery (current shipped behavior)

Crosspack executes install/upgrade/uninstall mutations under a transaction state machine with persisted typed status markers coordinated by installer-owned transaction APIs:

1. `planning`: resolve graph, artifact selection, and preflight checks.
2. `applying`: stage/extract/apply package and binary mutations.
3. `rolling_back` (only on failure/interruption): reverse applied steps to restore a consistent prefix.
4. `completed` or terminal failure marker after rollback attempt.

Rollback snapshot/replay contract (current behavior):

- per-package snapshots include package tree, receipt, exposed binaries, exposed package completions, exposed GUI assets, and optional native sidecar state,
- transaction metadata writes are atomic replacements, active marker writes/removals are durable and idempotent, journal appends are flushed before subsequent forward mutations are considered complete, and parent directory entries are synced where the platform supports it,
- rollback payloads are captured and journaled before destructive package lifecycle mutations; package apply `state=done` entries are journaled only after successful forward mutation,
- source-build package application journals explicit source phase steps (`source_fetch:*`, `source_build_system:*`, `source_install:*`) in addition to package apply steps, with `source_build_system:*` recorded only after successful source build execution,
- rollback replays compensating package steps in reverse journal order, including native step names (`install_native_package:<name>`, `upgrade_native_package:<name>`),
- native uninstall actions are replayed before managed snapshot restore for native package steps.

Recovery classification is deterministic and installer-owned:

- clean terminal state: no action,
- empty `planning`: cleanup stale planning metadata/staging,
- `planning` with payload or `applying`: require rollback,
- `committed` or legacy `completed`: finalize stale active marker,
- `rolling_back`: resume rollback,
- `rolled_back`: clear stale active marker,
- `failed` or unreadable/mismatched metadata/journal state: fail closed with repair-required reason code.

Operator commands:
- `rollback [txid]`: replay rollback for eligible interrupted/failed transactions.
- `repair`: clear stale markers and reconcile recoverable interrupted state; plain output includes additive `repair action=<code>` diagnostics.
- `doctor`: surface transaction health and prefix diagnostics; when trusted active metadata exists it also emits additive `transaction_detail txid=<txid> status=<status> operation=<operation> step=<step-or-none>`.

## Dependency Policy Behavior

Current behavior includes provider capability selection (`provides`), conflict gating (`conflicts`) during resolution/preflight, and replacement semantics (`replaces`) with ownership-aware binary handoff. Dependency tokens resolve to direct package manifests first; when no direct package exists, configured registry metadata is searched for packages declaring the requested capability. During upgrade planning, an already-installed provider at the same package version is preferred for a capability when it still satisfies constraints, pins, and conflicts; direct package-name dependencies still take precedence over provider candidates. `docs/dependency-policy-spec.md` remains the broader design reference for future policy expansion.

## Receipt Fields

- `name`
- `version`
- `target` (optional for backward compatibility)
- `artifact_url` (optional)
- `artifact_sha256` (optional)
- `cache_path` (optional)
- `exposed_bin` (repeated, optional)
- `exposed_completion` (repeated, optional)
- `install_mode` (`managed` or `native`; legacy receipts default to `managed`)
- `state/installed/<name>.gui` sidecar (optional): GUI asset ownership keys and storage paths for uninstall/upgrade cleanup.
- `state/installed/<name>.gui-native` sidecar (optional): native uninstall action records (`key`, `kind`, `path`) for deterministic uninstall/rollback cleanup.
- `state/installed/<name>.services` sidecar (optional): declared service records (`name`, optional `native_id`) for deterministic service command routing.
- `state/installed/<name>.integrations` sidecar (optional): declared Docker CLI, PATH plugin, and service projection records for deterministic status, activation, and cleanup.
- `state/installed/integrations.activation` state file (optional): versioned activation records with package identity, integration key, adapter, desired/applied state, host path, and reason code.
- `identity_profile`, `identity_target`, `identity_source_namespace`, `identity_source_provenance`, and `identity_package` (identity-keyed receipts)
- `state/installed/default--<target-or-host>--default--<name>.state.json` document (optional current format): versioned hydrated package state including identity, receipt, GUI/native/service/integration projections; legacy `<name>.state.json` and previous three-part identity-key documents remain readable.
- `dependency` (repeated `name@version`, optional)
- `install_reason` (`root` or `dependency`; legacy receipts default to `root`)
- `install_status` (`installed`)
- `installed_at_unix`

## Failure Handling

- Checksum mismatch: cached artifact is removed and install fails.
- Registry key/signature validation failure: install/upgrade and other metadata-dependent operations fail closed.
- Unsupported archive type (including removed `deb`/`rpm` kinds): install fails with actionable message.
- Unsupported constrained kind host (Windows-only native: `exe`, `msi`, `msix`, `appx`; macOS-only native: `pkg`; macOS-only managed: `dmg`; Linux-only managed: `appimage`): install fails with actionable message.
- Installer/package staging failures (`exe`, `msi`, `msix`, `appx`, `pkg`, `dmg`, `appimage`): install fails closed; Crosspack does not execute vendor installers as fallback.
- Package maintainer scripts are not executed for `pkg`; script-dependent installs fail closed.
- Extraction failure: temporary extraction directory is cleaned up best-effort.
- Incomplete download: `.part` file is removed on failed download.
- Binary collision: install fails if a requested binary is already owned by another package or exists unmanaged in `<prefix>/bin`.
- Completion collision: install fails if a projected package completion file is already owned by another package or exists unmanaged in Crosspack completion storage.
- GUI asset collision: install fails if a projected GUI ownership key is already owned by another package or a projected GUI asset path already exists unmanaged.
- Native GUI registration failures (including macOS destination prepare/write failures and unmanaged overwrite protection): install/upgrade/uninstall emit warnings and continue when package payload install/removal succeeded.
- Service install-time activation for `enable = true` is not shipped; manifests that request it fail closed before host mutation and do not persist activation state.
- Explicit `crosspack integrations enable|disable` records success/failure in the activation state file and never treats Docker CLI or PATH plugin projection alone as host activation.
- Native service adapter failures for `services status|start|stop|restart`: commands return deterministic fallback reason codes (`unsupported-host`, `adapter-tool-missing`, `native-command-failed`) while preserving deterministic plain output shape.
- Global solve downgrade requirement during `upgrade`: operation fails with an explicit downgrade message and command hint.
- Completion asset refresh failure: install/upgrade/uninstall warns but does not fail.

## Uninstall Flow

`crosspack uninstall <name>` executes this sequence:

1. Read all receipts and build a dependency graph from receipt dependencies.
2. Compute reachability from all remaining root receipts.
3. If target package is still reachable from any remaining root, block uninstall and report sorted blocking roots.
4. Otherwise remove the requested package and prune orphaned dependency closure no longer reachable from any remaining root.
5. For all removed packages:
   - if `install_mode=native`, run native uninstall actions from `.gui-native` sidecar before managed cleanup,
   - remove package directories, exposed binaries, exposed package completion files, and GUI assets,
   - for managed installs, remove native GUI registrations best-effort using `.gui-native` state records,
   - disable/remove supported owned activation records and host paths before deleting projected integration payloads; service activation records are preserved if host cleanup cannot be verified,
   - remove GUI sidecars (`.gui` and `.gui-native`), integration sidecars (`.integrations`), and receipt files,
   - collect cache paths from receipts.
6. Remove cache files that are no longer referenced by any remaining receipt.
7. Return deterministic uninstall result including status, pruned dependency names, and blocking roots (if blocked).

## Current Limits

- Pin constraints are simple per-package semver requirements stored as files.

## Upgrade and Pin

- `crosspack pin <name@constraint>` writes a pin at `<prefix>/state/pins/<name>.pin`.
- `crosspack install` and `crosspack upgrade` both enforce pin constraints during version selection.
- `crosspack upgrade <name[@constraint]>` upgrades one installed package if a newer compatible version exists.
- `crosspack upgrade` upgrades all installed root packages with one solve per target group, preserving each group's target triple from receipts.
- `crosspack upgrade` fails if grouped solves would touch the same package name across different targets; with current package-name keyed state, use separate prefixes for cross-target installs.
- If a package is already current (or only older/equal versions match constraints), upgrade reports it as up to date.

## Shell Setup and Completions

- `crosspack completions <bash|zsh|fish|powershell>` prints completion scripts to stdout.
- Completion generation targets the canonical `crosspack` command name and appends package completion loader logic for `<prefix>/share/completions/packages/<shell>/`.
- `crosspack init-shell [--shell <bash|zsh|fish|powershell>]` prints PATH + completion setup snippet; when `--shell` is omitted it auto-detects from `$SHELL` (Unix) and falls back to `bash` on Unix / `powershell` on Windows.
- Unix installer (`scripts/install.sh`) auto-detects shell from `$SHELL` (`bash`, `zsh`, or `fish`) and, by default:
  - writes completion scripts to `<prefix>/share/completions/crosspack.<shell>`,
  - creates or updates a single managed profile block in `~/.bashrc`, `~/.zshrc`, or `~/.config/fish/config.fish`,
  - evaluates `crosspack init-shell --shell <shell>` so PATH, completions, and package shell-init snippets stay in sync.
- Windows installer (`scripts/install.ps1`) writes PowerShell completion script to `<prefix>\share\completions\crosspack.ps1` and updates `$PROFILE.CurrentUserCurrentHost` with one managed block for PATH + completion sourcing.
- Installers resolve the default `core` fingerprint at runtime by downloading `registry.pub` from `https://github.com/spiritledsoftware/crosspack-registry` and hashing it (SHA-256).
- Installers fail closed on fetch/hash/validation errors.
- Installer fingerprint overrides remain available for controlled/offline scenarios (`CROSSPACK_CORE_FINGERPRINT` on Unix, `-CoreFingerprint` on Windows).
- Installer shell setup is best-effort: unsupported shells or profile write failures print warnings and manual commands, but installation still succeeds.
- Opt out of installer shell setup with:
  - Unix: `CROSSPACK_NO_SHELL_SETUP=1`
  - Windows: `-NoShellSetup`

## Forward-Looking Extensions

The current flow describes shipped behavior on the current release line. Broader design references are specified in:

- Dependency policy expansion beyond current provider/conflict/replacement behavior: `docs/dependency-policy-spec.md`.
- Transaction journal, rollback, and crash recovery policy expansion beyond current shipped rollback/repair behavior: `docs/transaction-rollback-spec.md`.

Related docs:
- Runtime architecture: `docs/architecture.md`
- Manifest field and signing semantics: `docs/manifest-spec.md`
