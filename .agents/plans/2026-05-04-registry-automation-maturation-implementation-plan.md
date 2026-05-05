# Registry Automation Maturation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mature registry automation into a rolling autonomous pipeline with durable state, package quarantine, package-scoped failure isolation, and root client hardening.

**Architecture:** The registry bot remains in the registry submodule and owns upstream discovery, deterministic metadata generation, committed automation state, package quarantine, and rolling PR updates. Signing remains the existing merge-time workflow. The root `crosspack-registry` crate hardens package reads so package-level poison does not block unrelated registry operations while source-level trust still fails closed.

**Tech Stack:** Python 3.11 registry automation scripts and tests in `registry/`; GitHub Actions workflows in `registry/.github/workflows/`; Rust `anyhow`, `toml`, and `crosspack-security` in `crates/crosspack-registry`; existing Cargo workspace checks.

---

## File Map

- Modify: `registry/scripts/upstream-release-bot.py`
  - Upgrade bot state from schema version 1 to 2.
  - Add package state, per-package/source backoff, quarantine add/update/clear helpers, package-scoped result accounting, rolling PR branch regeneration, and deterministic summary output.
- Modify: `registry/tests/test_upstream_release_bot.py`
  - Add tests for schema migration, state sorting, rate-limit backoff, quarantine behavior, valid regeneration clearance, and rolling PR command flow.
- Modify: `registry/.github/workflows/upstream-release-bot.yml`
  - Keep hourly schedule and dry-run support.
  - Run the bot with the rolling branch defaults and existing GitHub App token.
- Modify: `registry/scripts/registry-update-runbook.md`
  - Document rolling PR, quarantine, state file, merge-time signing, and recovery commands.
- Modify: `crates/crosspack-registry/src/registry_index.rs`
  - Add package-level skip diagnostics and tolerant list/search/provider behavior.
  - Keep direct selected package loads strict enough to fail the selected invalid package request.
- Modify: `crates/crosspack-registry/src/source_sync.rs`
  - Split fatal source trust validation from skippable signed package semantic validation so one signed malformed package does not block snapshot readiness.
- Modify: `crates/crosspack-registry/src/tests.rs`
  - Add source sync and broad-read poison-isolation regression tests, plus fail-closed trust regression tests.
- Modify: `crates/crosspack-registry/src/lib.rs`
  - Re-export package skip diagnostics only if needed by CLI warnings.
- Modify: `crates/crosspack-cli/src/metadata.rs`
  - Return search/provider diagnostics from broad metadata commands.
- Modify: `crates/crosspack-cli/src/dispatch.rs` or the included CLI dispatch section that handles `Commands::Search`
  - Print additive warnings for skipped package-level poison if `crosspack-registry` exposes diagnostics.
- Modify: `README.md` and `docs/registry-spec.md`
  - Document package-level poison isolation without weakening source-level trust guarantees.

---

## Task 1: Bot State Schema V2

**Files:**
- Modify: `registry/scripts/upstream-release-bot.py`
- Modify: `registry/tests/test_upstream_release_bot.py`
- Fixture state path: `registry/state/upstream-release-bot.json`

- [ ] **Step 1: Write failing migration and deterministic write tests**

Add these tests to `UpstreamReleaseBotTests` in `registry/tests/test_upstream_release_bot.py` after `test_write_bot_state_sorts_keys_and_creates_parent`:

```python
    def test_load_bot_state_migrates_v1_sources_to_v2(self) -> None:
        with tempfile.TemporaryDirectory(prefix="release-bot-state-") as tmp:
            state_path = Path(tmp) / "state" / "upstream-release-bot.json"
            state_path.parent.mkdir(parents=True)
            state_path.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "sources": {
                            "github_releases:BurntSushi/ripgrep": {
                                "etag": "abc",
                                "latest_version": "15.2.0",
                            }
                        },
                    }
                )
                + "\n",
                encoding="utf-8",
            )

            state = self.bot.load_bot_state(state_path)

        self.assertEqual(state["schema_version"], 2)
        self.assertEqual(
            state["sources"]["github_releases:BurntSushi/ripgrep"]["etag"], "abc"
        )
        self.assertEqual(state["packages"], {})
        self.assertEqual(state["quarantine"], {})

    def test_write_bot_state_v2_sorts_all_top_level_maps(self) -> None:
        with tempfile.TemporaryDirectory(prefix="release-bot-state-") as tmp:
            state_path = Path(tmp) / "state" / "upstream-release-bot.json"

            self.bot.write_bot_state(
                state_path,
                {
                    "schema_version": 2,
                    "sources": {"z": {"etag": "z"}, "a": {"etag": "a"}},
                    "packages": {"zpkg": {"latest_version": "2.0.0"}, "apkg": {}},
                    "quarantine": {"zpkg": {"reason_code": "metadata-malformed"}, "apkg": {}},
                },
            )

            written = json.loads(state_path.read_text(encoding="utf-8"))

        self.assertEqual(list(written.keys()), ["packages", "quarantine", "schema_version", "sources"])
        self.assertEqual(list(written["sources"].keys()), ["a", "z"])
        self.assertEqual(list(written["packages"].keys()), ["apkg", "zpkg"])
        self.assertEqual(list(written["quarantine"].keys()), ["apkg", "zpkg"])
```

- [ ] **Step 2: Run tests and confirm failure**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_load_bot_state_migrates_v1_sources_to_v2 tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_write_bot_state_v2_sorts_all_top_level_maps
```

Expected: failure because `STATE_SCHEMA_VERSION` is still `1`, `packages` and `quarantine` are absent, and V1 state is rejected instead of migrated.

- [ ] **Step 3: Implement schema V2 helpers**

In `registry/scripts/upstream-release-bot.py`, change the constant and replace `empty_bot_state`, `load_bot_state`, and `write_bot_state` with this implementation:

```python
STATE_SCHEMA_VERSION = 2


def empty_bot_state() -> dict[str, Any]:
    return {"schema_version": STATE_SCHEMA_VERSION, "sources": {}, "packages": {}, "quarantine": {}}


def _object_map(value: object) -> dict[str, dict[str, Any]]:
    if not isinstance(value, dict):
        return {}
    return {
        key: entry
        for key, entry in value.items()
        if isinstance(key, str) and isinstance(entry, dict)
    }


def _normalize_bot_state(data: dict[str, Any]) -> dict[str, Any] | None:
    schema_version = data.get("schema_version")
    if schema_version == 1:
        return {
            "schema_version": STATE_SCHEMA_VERSION,
            "sources": _object_map(data.get("sources")),
            "packages": {},
            "quarantine": {},
        }
    if schema_version != STATE_SCHEMA_VERSION:
        return None
    return {
        "schema_version": STATE_SCHEMA_VERSION,
        "sources": _object_map(data.get("sources")),
        "packages": _object_map(data.get("packages")),
        "quarantine": _object_map(data.get("quarantine")),
    }


def load_bot_state(path: Path) -> dict[str, Any]:
    if not path.exists():
        return empty_bot_state()
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"Ignoring invalid release bot state at {path}: {exc}", file=sys.stderr)
        return empty_bot_state()
    if not isinstance(data, dict):
        print(f"Ignoring invalid release bot state at {path}: root must be an object", file=sys.stderr)
        return empty_bot_state()
    normalized = _normalize_bot_state(data)
    if normalized is None:
        print(f"Ignoring unsupported release bot state at {path}", file=sys.stderr)
        return empty_bot_state()
    return normalized


def write_bot_state(path: Path, state: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    normalized = {
        "packages": dict(sorted(_object_map(state.get("packages")).items())),
        "quarantine": dict(sorted(_object_map(state.get("quarantine")).items())),
        "schema_version": STATE_SCHEMA_VERSION,
        "sources": dict(sorted(_object_map(state.get("sources")).items())),
    }
    path.write_text(
        json.dumps(normalized, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
```

- [ ] **Step 4: Update existing tests expecting V1 empty state**

In `registry/tests/test_upstream_release_bot.py`, replace expected empty states with:

```python
{"schema_version": 2, "sources": {}, "packages": {}, "quarantine": {}}
```

Update `test_write_bot_state_sorts_keys_and_creates_parent` to include empty `packages` and `quarantine` in the expected JSON.

- [ ] **Step 5: Run focused tests**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_load_bot_state_returns_empty_state_when_missing tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_load_bot_state_warns_and_rebuilds_when_invalid tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_load_bot_state_migrates_v1_sources_to_v2 tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_write_bot_state_sorts_keys_and_creates_parent tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_write_bot_state_v2_sorts_all_top_level_maps
```

Expected: all selected tests pass.

- [ ] **Step 6: Commit**

```bash
git -C registry add scripts/upstream-release-bot.py tests/test_upstream_release_bot.py
git -C registry commit -m "feat(bot): add registry automation state v2"
```

---

## Task 2: Per-Package Backoff And Reason Codes

**Files:**
- Modify: `registry/scripts/upstream-release-bot.py`
- Modify: `registry/tests/test_upstream_release_bot.py`

- [ ] **Step 1: Write failing backoff tests**

Add these tests to `UpstreamReleaseBotTests`:

```python
    def test_rate_limit_reset_header_sets_backoff_until(self) -> None:
        headers = Message()
        headers["x-ratelimit-reset"] = "1770000000"
        error = urllib.error.HTTPError("https://api.github.com/repos/o/r/releases", 403, "rate limit", headers, None)

        backoff = self.bot.backoff_from_http_error(error, now_epoch=1769999900)

        self.assertEqual(backoff["reason_code"], "rate-limited")
        self.assertEqual(backoff["backoff_until"], "2026-02-02T02:40:00Z")

    def test_should_skip_package_when_backoff_is_active(self) -> None:
        entry = {"backoff_until": "2099-01-01T00:00:00Z"}

        self.assertTrue(self.bot.package_backoff_active(entry, now_iso="2026-05-04T12:00:00Z"))

    def test_should_not_skip_package_when_backoff_expired(self) -> None:
        entry = {"backoff_until": "2026-05-04T11:59:59Z"}

        self.assertFalse(self.bot.package_backoff_active(entry, now_iso="2026-05-04T12:00:00Z"))
```

- [ ] **Step 2: Run tests and confirm failure**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_rate_limit_reset_header_sets_backoff_until tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_should_skip_package_when_backoff_is_active tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_should_not_skip_package_when_backoff_expired
```

Expected: failure because helper functions do not exist.

- [ ] **Step 3: Implement helper functions**

Add this code after `utc_now_iso` in `registry/scripts/upstream-release-bot.py`:

```python
def iso_from_epoch(epoch: int) -> str:
    return datetime.fromtimestamp(epoch, timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def package_backoff_active(package_state: dict[str, Any], *, now_iso: str | None = None) -> bool:
    backoff_until = package_state.get("backoff_until")
    if not isinstance(backoff_until, str) or not backoff_until:
        return False
    current = now_iso or utc_now_iso()
    return backoff_until > current


def backoff_from_http_error(error: urllib.error.HTTPError, *, now_epoch: int | None = None) -> dict[str, Any]:
    reset = error.headers.get("x-ratelimit-reset") if error.headers else None
    if isinstance(reset, str) and reset.isdigit():
        reset_epoch = int(reset)
    else:
        base = int(time.time() if now_epoch is None else now_epoch)
        reset_epoch = base + 3600
    return {
        "reason_code": "rate-limited",
        "detail": _format_fetch_error(error),
        "backoff_until": iso_from_epoch(reset_epoch),
        "last_failed_at": utc_now_iso(),
    }
```

- [ ] **Step 4: Wire package state into the main loop**

In `main`, after `state_sources` setup, initialize package state:

```python
    state_packages = bot_state.setdefault("packages", {})
    if not isinstance(state_packages, dict):
        state_packages = {}
        bot_state["packages"] = state_packages
```

Inside the `for config_path in config_paths:` loop, after loading `config`, derive `package_name` and skip active backoff:

```python
        package_name = str(config.get("name") or config_path.stem)
        package_state = state_packages.setdefault(package_name, {})
        if not isinstance(package_state, dict):
            package_state = {}
            state_packages[package_name] = package_state
        if package_backoff_active(package_state):
            skipped_fetches += 1
            print(
                f"registry_update package={package_name} status=skipped reason=backoff-active reset_at={package_state.get('backoff_until')}",
                file=sys.stderr,
            )
            continue
```

In the `except urllib.error.HTTPError as error:` skippable branch, update package state before continuing:

```python
            package_state.update(backoff_from_http_error(error))
            state_changed = True
```

After successful fetch for any release kind, clear stale backoff fields:

```python
            for key in ("backoff_until", "reason_code", "detail", "last_failed_at"):
                if key in package_state:
                    package_state.pop(key, None)
                    state_changed = True
```

- [ ] **Step 5: Run focused tests**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_rate_limit_reset_header_sets_backoff_until tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_should_skip_package_when_backoff_is_active tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_should_not_skip_package_when_backoff_expired
```

Expected: all selected tests pass.

- [ ] **Step 6: Commit**

```bash
git -C registry add scripts/upstream-release-bot.py tests/test_upstream_release_bot.py
git -C registry commit -m "feat(bot): add per-package upstream backoff"
```

---

## Task 3: Quarantine Malformed Generated Packages

**Files:**
- Modify: `registry/scripts/upstream-release-bot.py`
- Modify: `registry/tests/test_upstream_release_bot.py`

- [ ] **Step 1: Write failing quarantine tests**

Add these tests:

```python
    def test_quarantine_update_preserves_first_seen(self) -> None:
        state = self.bot.empty_bot_state()
        quarantine = state["quarantine"]
        quarantine["zig"] = {
            "reason_code": "metadata-malformed",
            "first_seen_at": "2026-05-04T10:00:00Z",
            "last_seen_at": "2026-05-04T10:00:00Z",
            "attempted_version": "0.16.0",
            "last_good_version": "0.15.2",
        }

        changed = self.bot.quarantine_package(
            state,
            package="zig",
            reason_code="metadata-malformed",
            detail="missing artifact url",
            attempted_version="0.16.1",
            last_good_version="0.15.2",
            now_iso="2026-05-04T11:00:00Z",
        )

        self.assertTrue(changed)
        self.assertEqual(quarantine["zig"]["first_seen_at"], "2026-05-04T10:00:00Z")
        self.assertEqual(quarantine["zig"]["last_seen_at"], "2026-05-04T11:00:00Z")
        self.assertEqual(quarantine["zig"]["attempted_version"], "0.16.1")

    def test_clear_quarantine_returns_true_only_when_entry_exists(self) -> None:
        state = self.bot.empty_bot_state()
        state["quarantine"]["zig"] = {"reason_code": "metadata-malformed"}

        self.assertTrue(self.bot.clear_quarantine(state, package="zig"))
        self.assertFalse(self.bot.clear_quarantine(state, package="zig"))
        self.assertNotIn("zig", state["quarantine"])
```

- [ ] **Step 2: Run tests and confirm failure**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_quarantine_update_preserves_first_seen tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_clear_quarantine_returns_true_only_when_entry_exists
```

Expected: failure because quarantine helpers do not exist.

- [ ] **Step 3: Implement quarantine helpers**

Add this code after the backoff helpers:

```python
def quarantine_package(
    state: dict[str, Any],
    *,
    package: str,
    reason_code: str,
    detail: str,
    attempted_version: str,
    last_good_version: str | None,
    now_iso: str | None = None,
) -> bool:
    quarantine = state.setdefault("quarantine", {})
    if not isinstance(quarantine, dict):
        quarantine = {}
        state["quarantine"] = quarantine
    now_value = now_iso or utc_now_iso()
    previous = quarantine.get(package)
    first_seen = previous.get("first_seen_at") if isinstance(previous, dict) else None
    next_entry = {
        "reason_code": reason_code,
        "detail": detail,
        "first_seen_at": first_seen if isinstance(first_seen, str) else now_value,
        "last_seen_at": now_value,
        "attempted_version": attempted_version,
    }
    if last_good_version is not None:
        next_entry["last_good_version"] = last_good_version
    changed = previous != next_entry
    quarantine[package] = next_entry
    return changed


def clear_quarantine(state: dict[str, Any], *, package: str) -> bool:
    quarantine = state.setdefault("quarantine", {})
    if not isinstance(quarantine, dict):
        state["quarantine"] = {}
        return False
    return quarantine.pop(package, None) is not None
```

- [ ] **Step 4: Wire quarantine around generation failures without reusing stale package state**

In the `except generator.GenerateError as exc:` block, replace the current skip-only behavior with:

```python
            package_state = state_packages.setdefault(update.package, {})
            if not isinstance(package_state, dict):
                package_state = {}
                state_packages[update.package] = package_state
            last_good_version = package_state.get("last_successful_version")
            if not isinstance(last_good_version, str):
                last_good_version = None
            if quarantine_package(
                bot_state,
                package=update.package,
                reason_code="metadata-malformed",
                detail=str(exc),
                attempted_version=update.version,
                last_good_version=last_good_version,
            ):
                state_changed = True
            skipped_updates += 1
            print(
                f"registry_update package={update.package} status=quarantined reason=metadata-malformed attempted={update.version}",
                file=sys.stderr,
            )
            continue
```

Do not clear quarantine immediately after writing files. Quarantine clears only after package-scoped validation succeeds in Task 4.

- [ ] **Step 5: Add package-scoped validation helper for generated paths**

Add this helper next to `validate_generated_paths`:

```python
def validate_package_generated_paths(*, repo_root: Path, package: str, staged_paths: list[Path]) -> None:
    package_paths = [path for path in staged_paths if path == Path("packages") / f"{package}.toml"]
    release_paths = [
        path
        for path in staged_paths
        if len(path.parts) == 3
        and path.parts[0] == "releases"
        and path.parts[1] == package
        and path.suffix == ".toml"
    ]
    validate_generated_paths(repo_root=repo_root, staged_paths=[*package_paths, *release_paths])
```

In `main`, after generating each package's candidate files and before adding them to the rolling PR path list, validate only that package. On validation failure, quarantine that package, delete/revert its generated candidate paths, and continue unrelated packages:

```python
        package_state = state_packages.setdefault(update.package, {})
        if not isinstance(package_state, dict):
            package_state = {}
            state_packages[update.package] = package_state
        try:
            validate_package_generated_paths(
                repo_root=repo_root,
                package=update.package,
                staged_paths=staged_paths,
            )
        except Exception as exc:
            for generated_path in staged_paths:
                full_path = repo_root / generated_path
                if full_path.exists():
                    full_path.unlink()
            last_good_version = package_state.get("last_successful_version")
            if not isinstance(last_good_version, str):
                last_good_version = None
            if quarantine_package(
                bot_state,
                package=update.package,
                reason_code="metadata-malformed",
                detail=str(exc),
                attempted_version=update.version,
                last_good_version=last_good_version,
            ):
                state_changed = True
            quarantined_packages.append(update.package)
            skipped_updates += 1
            print(
                f"registry_update package={update.package} status=quarantined reason=metadata-malformed attempted={update.version}",
                file=sys.stderr,
            )
            continue
```

Only after that validation succeeds, update package success state and clear quarantine:

```python
        package_state["last_successful_version"] = update.version
        package_state["last_generated_at"] = utc_now_iso()
        if clear_quarantine(bot_state, package=update.package):
            state_changed = True
```

- [ ] **Step 6: Ensure state-only quarantine changes are written**

Remove the existing early return that exits when `planned` is empty before state write/summary/PR handling. Replace it with state-aware control flow:

```python
    if not planned and not state_changed:
        print(
            "registry_update_summary updated=0 up_to_date=0 quarantined=0 transient_failed=0 "
            f"skipped={skipped_fetches}"
        )
        return 0

    if state_changed and not args.dry_run:
        write_bot_state(args.state_path, bot_state)
```

When `args.create_prs` is true, include `args.state_path` in the rolling PR staged paths from Task 4 even if no package/release files changed.

- [ ] **Step 7: Run focused tests**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_quarantine_update_preserves_first_seen tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_clear_quarantine_returns_true_only_when_entry_exists
```

Expected: all selected tests pass.

- [ ] **Step 8: Commit**

```bash
git -C registry add scripts/upstream-release-bot.py tests/test_upstream_release_bot.py
git -C registry commit -m "feat(bot): quarantine malformed package updates"
```

---

## Task 4: Rolling Bot PR Regenerated From Main

**Files:**
- Modify: `registry/scripts/upstream-release-bot.py`
- Modify: `registry/tests/test_upstream_release_bot.py`
- Modify: `registry/.github/workflows/upstream-release-bot.yml`

- [ ] **Step 1: Write failing command-flow test**

Add this test. It mocks `_run` so no network or GitHub calls execute:

```python
    def test_open_or_update_rolling_pr_regenerates_branch_from_base(self) -> None:
        calls: list[list[str]] = []

        def fake_run(cmd, *, cwd):
            calls.append(cmd)
            stdout = ""
            if cmd[:4] == ["gh", "pr", "list", "--head"]:
                stdout = "[]"
            return subprocess.CompletedProcess(cmd, 0, stdout=stdout, stderr="")

        with tempfile.TemporaryDirectory(prefix="release-bot-pr-") as tmp:
            repo = Path(tmp)
            (repo / "packages").mkdir()
            (repo / "packages" / "ripgrep.toml").write_text("name = \"ripgrep\"\n", encoding="utf-8")
            with mock.patch.object(self.bot, "_run", side_effect=fake_run), mock.patch.object(
                self.bot, "validate_generated_paths"
            ):
                self.bot._open_or_update_rolling_pr(
                    repo_root=repo,
                    staged_paths=[Path("packages/ripgrep.toml")],
                    base_branch="main",
                    branch_name="upstream-release/rolling",
                    title="chore(registry): update upstream releases",
                    body="## Summary\n- test\n",
                )

        self.assertIn(["git", "fetch", "origin", "main"], calls)
        self.assertIn(["git", "switch", "-C", "upstream-release/rolling", "origin/main"], calls)
        self.assertIn(["git", "push", "--force-with-lease", "-u", "origin", "upstream-release/rolling"], calls)
```

- [ ] **Step 2: Run test and confirm failure**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_open_or_update_rolling_pr_regenerates_branch_from_base
```

Expected: failure because `_open_or_update_rolling_pr` does not exist.

- [ ] **Step 3: Replace per-package PR helper with rolling PR helper**

In `registry/scripts/upstream-release-bot.py`, add this helper after `_open_pr_number_for_branch` and stop calling `_open_or_update_pr` from `main`:

```python
def _open_or_update_rolling_pr(
    *,
    repo_root: Path,
    staged_paths: list[Path],
    base_branch: str,
    branch_name: str,
    title: str,
    body: str,
) -> None:
    path_snapshot = _snapshot_paths(repo_root, staged_paths)
    _run(["git", "fetch", "origin", base_branch], cwd=repo_root)
    _run(["git", "switch", "-C", branch_name, f"origin/{base_branch}"], cwd=repo_root)
    _restore_path_snapshot(repo_root, path_snapshot)
    if staged_paths:
        _run(["git", "add", *(str(path) for path in staged_paths)], cwd=repo_root)
    validate_generated_paths(repo_root=repo_root, staged_paths=staged_paths)
    staged = _run(["git", "diff", "--cached", "--name-only"], cwd=repo_root)
    if not staged.stdout.strip():
        number = _open_pr_number_for_branch(repo_root=repo_root, branch_name=branch_name)
        if number is not None:
            _enable_pr_automerge(repo_root=repo_root, pr_ref=str(number))
            print(f"PR already open for {branch_name}; enabled automerge")
        return
    _run(["git", "commit", "-m", title], cwd=repo_root)
    _run(["git", "push", "--force-with-lease", "-u", "origin", branch_name], cwd=repo_root)
    number = _open_pr_number_for_branch(repo_root=repo_root, branch_name=branch_name)
    if number is not None:
        _run(["gh", "pr", "edit", str(number), "--title", title, "--body", body], cwd=repo_root)
        _enable_pr_automerge(repo_root=repo_root, pr_ref=str(number))
        print(f"Updated PR #{number} for {branch_name}; enabled automerge")
        return
    _run(
        [
            "gh",
            "pr",
            "create",
            "--base",
            base_branch,
            "--head",
            branch_name,
            "--title",
            title,
            "--body",
            body,
        ],
        cwd=repo_root,
    )
    _enable_pr_automerge(repo_root=repo_root, pr_ref=branch_name)
```

- [ ] **Step 4: Accumulate staged paths and summary once per run**

Before loading state or generating metadata in `main`, reset create-PR runs to current `origin/<base>` so discovery and generation happen against the same tree that will become the rolling branch:

```python
    if args.create_prs:
        _run(["git", "fetch", "origin", args.base_branch], cwd=repo_root)
        _run(["git", "switch", "-C", args.branch_name, f"origin/{args.base_branch}"], cwd=repo_root)
```

Keep `_open_or_update_rolling_pr`'s fetch/switch as a final safety reset before restoring the generated path snapshot.

In `main`, create these counters before the generation loop:

```python
    all_staged_paths: list[Path] = []
    updated_packages: list[str] = []
    quarantined_packages: list[str] = []
```

When appending a package or release path, append to `all_staged_paths` if absent:

```python
        for path in staged_paths:
            if path not in all_staged_paths:
                all_staged_paths.append(path)
```

When a package is successfully generated, append `f"{update.package}@{update.version}"` to `updated_packages`. When quarantine occurs, append the package name to `quarantined_packages`.

After generation and PR handling, replace the legacy final summary with the stable machine-readable summary required by the spec:

```python
    print(
        "registry_update_summary "
        f"updated={created_releases} "
        "up_to_date=0 "
        f"quarantined={len(set(quarantined_packages))} "
        f"transient_failed={skipped_updates} "
        f"skipped={skipped_fetches}"
    )
```

After the generation loop, if `state_changed`, write state and add `args.state_path` to `all_staged_paths`. If `args.create_prs` and `all_staged_paths` is non-empty, call `_open_or_update_rolling_pr` once:

```python
    if state_changed and not args.dry_run:
        write_bot_state(args.state_path, bot_state)
        if args.state_path not in all_staged_paths:
            all_staged_paths.append(args.state_path)

    if args.create_prs and all_staged_paths:
        title = "chore(registry): update upstream releases"
        body = render_pr_body(
            updated_packages=updated_packages,
            quarantined_packages=quarantined_packages,
            created_releases=created_releases,
            written_packages=written_packages,
            skipped_updates=skipped_updates,
            skipped_fetches=skipped_fetches,
        )
        _open_or_update_rolling_pr(
            repo_root=repo_root,
            staged_paths=all_staged_paths,
            base_branch=args.base_branch,
            branch_name=args.branch_name,
            title=title,
            body=body,
        )
```

Add parser argument:

```python
    parser.add_argument("--branch-name", default="upstream-release/rolling")
```

Keep `--branch-prefix` for one release as a hidden/deprecated argument only if existing tests need it:

```python
    parser.add_argument("--branch-prefix", default=None, help=argparse.SUPPRESS)
```

- [ ] **Step 5: Add deterministic PR body renderer**

Add this helper near PR helpers:

```python
def render_pr_body(
    *,
    updated_packages: list[str],
    quarantined_packages: list[str],
    created_releases: int,
    written_packages: int,
    skipped_updates: int,
    skipped_fetches: int,
) -> str:
    updated = ", ".join(sorted(updated_packages)) if updated_packages else "none"
    quarantined = ", ".join(sorted(set(quarantined_packages))) if quarantined_packages else "none"
    return (
        "## Summary\n"
        f"- updated packages: {updated}\n"
        f"- quarantined packages: {quarantined}\n"
        f"- release manifests written: {created_releases}\n"
        f"- package templates updated: {written_packages}\n"
        f"- incomplete updates skipped: {skipped_updates}\n"
        f"- release fetches skipped: {skipped_fetches}\n"
        "\n## Validation\n"
        "- registry-validate-source.py for changed package templates\n"
        "- registry-validate.py --allow-missing-signatures for changed metadata\n"
    )
```

- [ ] **Step 6: Update workflow branch behavior**

In `registry/.github/workflows/upstream-release-bot.yml`, change the create PR command from:

```bash
args=(--create-prs)
```

to:

```bash
args=(--create-prs --branch-name upstream-release/rolling)
```

- [ ] **Step 7: Run focused tests**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot.UpstreamReleaseBotTests.test_open_or_update_rolling_pr_regenerates_branch_from_base
```

Expected: selected test passes.

- [ ] **Step 8: Commit**

```bash
git -C registry add scripts/upstream-release-bot.py tests/test_upstream_release_bot.py .github/workflows/upstream-release-bot.yml
git -C registry commit -m "feat(bot): use rolling upstream release PR"
```

---

## Task 5: Package-Level Poison Isolation In Registry Reader

**Files:**
- Modify: `crates/crosspack-registry/src/registry_index.rs`
- Modify: `crates/crosspack-registry/src/lib.rs` only if diagnostics need export
- Modify: `crates/crosspack-registry/src/tests.rs`

- [ ] **Step 1: Write failing Rust tests**

Add this test near the existing `package_versions_*` tests in `crates/crosspack-registry/src/tests.rs`. It uses the existing signing helpers so source-level signature checks remain active while the bad package fails at package metadata parsing.

```rust
#[test]
fn search_names_skips_poisoned_package_records() {
    let root = test_registry_root();
    let signing_key = signing_key();
    fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
        .expect("must write registry public key");

    let good_dir = root.join("releases").join("good");
    write_signed_package_template(
        &root,
        &signing_key,
        "good",
        &package_template_toml_with_license("good", "MIT"),
    );
    write_signed_release_manifest(&good_dir, &signing_key, "1.0.0", &release_toml("1.0.0"));

    let bad_dir = root.join("releases").join("bad");
    write_signed_package_template(&root, &signing_key, "bad", "name = [\"bad\"\n");
    write_signed_release_manifest(&bad_dir, &signing_key, "1.0.0", &release_toml("1.0.0"));

    let index = RegistryIndex::open(&root);
    let (names, diagnostics) = index
        .search_names_with_diagnostics("")
        .expect("must search while skipping poisoned package");

    assert_eq!(names, vec!["good".to_string()]);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].package, "bad");
    assert_eq!(diagnostics[0].reason_code, "package-metadata-invalid");

    let direct_error = index
        .package_versions("bad")
        .expect_err("direct selected package load must remain strict");
    assert!(direct_error.to_string().contains("failed parsing package template"));

    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Run test and confirm failure**

Run from workspace root:

```bash
cargo test -p crosspack-registry search_names_skips_poisoned_package_records -- --test-threads=1
```

Expected: failure because tolerant helper does not exist and current package loading fails on invalid package records.

- [ ] **Step 3: Add package skip diagnostic type**

Add near the structs at the top of `registry_index.rs`:

```rust
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PackageSkipDiagnostic {
    pub package: String,
    pub reason_code: &'static str,
    pub detail: String,
}
```

- [ ] **Step 4: Add classified tolerant package loading for list/search/provider paths**

Add a fatal/skippable classifier and private helper methods inside `impl RegistryIndex`. The classifier is deliberately conservative: signature/key/source trust failures stay fatal, and only parse/schema/merged-manifest content errors become skippable package diagnostics.

```rust
fn is_skippable_package_poison(error: &anyhow::Error) -> bool {
    let rendered = error.to_string();
    if rendered.contains("signature")
        || rendered.contains("trusted registry key")
        || rendered.contains("registry.pub")
        || rendered.contains("fingerprint")
    {
        return false;
    }
    rendered.contains("failed parsing package template")
        || rendered.contains("failed parsing release metadata")
        || rendered.contains("failed parsing merged manifest")
        || rendered.contains("failed serializing merged manifest")
        || rendered.contains("expected TOML table")
        || rendered.contains("not valid UTF-8")
}

    fn package_versions_tolerant(
        &self,
        package: &str,
        diagnostics: &mut Vec<PackageSkipDiagnostic>,
    ) -> Result<Vec<PackageManifest>> {
        match self.package_versions(package) {
            Ok(manifests) => Ok(manifests),
            Err(error) => {
                if !is_skippable_package_poison(&error) {
                    return Err(error);
                }
                diagnostics.push(PackageSkipDiagnostic {
                    package: package.to_string(),
                    reason_code: "package-metadata-invalid",
                    detail: error.to_string(),
                });
                Ok(Vec::new())
            }
        }
    }

    pub fn search_names_with_diagnostics(
        &self,
        needle: &str,
    ) -> Result<(Vec<String>, Vec<PackageSkipDiagnostic>)> {
        let mut names = Vec::new();
        let mut diagnostics = Vec::new();
        for name in self.package_names()? {
            if name.contains(needle) {
                let manifests = self.package_versions_tolerant(&name, &mut diagnostics)?;
                if !manifests.is_empty() {
                    names.push(name);
                }
            }
        }
        names.sort();
        Ok((names, diagnostics))
    }
```

Change `search_names` to call `search_names_with_diagnostics` and return only names.

- [ ] **Step 5: Keep selected package loads strict**

Do not change `RegistryIndex::package_versions(package)` semantics for direct package selection. If a user requests the malformed package directly, it should still return the direct error for that package.

- [ ] **Step 6: Add configured index diagnostics**

Add a `ConfiguredRegistryIndex::search_names_with_diagnostics` method that dedupes names and accumulates diagnostics across sources. Keep `ConfiguredRegistryIndex::search_names` returning names only.

```rust
    pub fn search_names_with_diagnostics(
        &self,
        needle: &str,
    ) -> Result<(Vec<String>, Vec<PackageSkipDiagnostic>)> {
        let mut deduped = HashSet::new();
        let mut diagnostics = Vec::new();
        for source in &self.sources {
            let (names, mut source_diagnostics) = source.index.search_names_with_diagnostics(needle)?;
            diagnostics.append(&mut source_diagnostics);
            for name in names {
                deduped.insert(name);
            }
        }
        let mut names: Vec<String> = deduped.into_iter().collect();
        names.sort();
        Ok((names, diagnostics))
    }
```

- [ ] **Step 7: Add tolerant provider/dependency broad reads**

Add `RegistryIndex::provider_versions_with_diagnostics` and `ConfiguredRegistryIndex::provider_versions_with_diagnostics` using `package_versions_tolerant` inside the existing package-name loop. Keep existing `provider_versions` returning only manifests by delegating to the diagnostic method and dropping diagnostics.

```rust
    pub fn provider_versions_with_diagnostics(
        &self,
        capability: &str,
    ) -> Result<(Vec<PackageManifest>, Vec<PackageSkipDiagnostic>)> {
        let mut providers = Vec::new();
        let mut diagnostics = Vec::new();
        for package_name in self.package_names()? {
            if !self.package_mentions_capability(&package_name, capability)? {
                continue;
            }
            providers.extend(
                self.package_versions_tolerant(&package_name, &mut diagnostics)?
                    .into_iter()
                    .filter(|manifest| manifest.provides.iter().any(|provided| provided == capability)),
            );
        }
        Ok((providers, diagnostics))
    }
```

If `package_mentions_capability` hits signed malformed TOML while scanning broad provider candidates, classify it the same way: skippable package poison becomes a diagnostic; signature/key failures return `Err`.

Add tests proving a malformed unrelated provider candidate does not block a valid provider result. Also add fail-closed regression tests proving broad search/provider still fails for missing `registry.pub`, missing package signature, invalid package signature, missing release signature, and invalid release signature.

- [ ] **Step 8: Run focused tests**

Run:

```bash
cargo test -p crosspack-registry search_names_skips_poisoned_package_records -- --test-threads=1
```

Expected: selected test passes. The fixture must keep valid signatures for both package files so the bad package proves package-level TOML poison isolation, not signature bypass.

- [ ] **Step 9: Commit**

```bash
git add crates/crosspack-registry/src/registry_index.rs crates/crosspack-registry/src/lib.rs
git commit -m "fix(registry): isolate invalid package records during search"
```

---

## Task 6: Source Sync Allows Signed Package Poison

**Files:**
- Modify: `crates/crosspack-registry/src/source_sync.rs`
- Modify: `crates/crosspack-registry/src/tests.rs`

- [ ] **Step 1: Write failing source readiness test**

Add a test in `crates/crosspack-registry/src/tests.rs` near existing source sync signature tests. The test should create a filesystem source with:

- valid `registry.pub`,
- valid signatures for every `packages/*.toml` and `releases/*/*.toml`,
- one valid package,
- one signed malformed package template or signed malformed merged manifest.

Expected assertion:

```rust
let updated = store
    .update_sources(&["official".to_string()])
    .expect("signed package poison must not block source readiness");
assert_eq!(updated.len(), 1);
let cache_root = root.join("cache").join("official");
assert!(cache_root.join("snapshot.json").exists());
assert!(source_has_ready_snapshot(&cache_root).expect("must read snapshot"));
```

Also assert that broad search on the ready cache returns the good package and reports one `PackageSkipDiagnostic` for the bad package.

- [ ] **Step 2: Run test and confirm failure**

Run:

```bash
cargo test -p crosspack-registry update_filesystem_source_accepts_signed_package_poison -- --test-threads=1
```

Expected: failure because `verify_metadata_signature_policy` currently calls `RegistryIndex::package_versions` and fails the whole source on the malformed package.

- [ ] **Step 3: Split source trust validation from semantic package parsing**

In `crates/crosspack-registry/src/source_sync.rs`, change `verify_metadata_signature_policy` so it verifies bytes and sidecars for all package templates and release manifests without requiring merged manifests to parse. It should still fail for missing/invalid signatures.

Implementation shape:

```rust
fn verify_metadata_signature_policy(staged_root: &Path, source_name: &str) -> Result<()> {
    let trusted_key_path = staged_root.join("registry.pub");
    let trusted_public_key_hex = fs::read_to_string(&trusted_key_path).with_context(|| {
        format!("source-metadata-invalid: source '{source_name}' missing registry.pub")
    })?;
    let trusted_public_key_hex = trusted_public_key_hex.trim();

    for document_path in signed_metadata_documents(staged_root)? {
        let document_bytes = fs::read(&document_path).with_context(|| {
            format!("source-metadata-invalid: source '{source_name}' failed reading {}", document_path.display())
        })?;
        verify_signed_metadata_document(
            source_name,
            &document_path,
            &document_bytes,
            trusted_public_key_hex,
        )?;
    }
    Ok(())
}
```

Use existing signature verification helper logic where possible instead of duplicating crypto primitives. Keep community recipe catalog policy unchanged and fatal.

- [ ] **Step 4: Preserve fail-closed source trust tests**

Add or update tests for these fatal cases during source sync:

- missing `registry.pub`,
- missing package template `.toml.sig`,
- invalid package template `.toml.sig`,
- missing release `.toml.sig`,
- invalid release `.toml.sig`,
- invalid configured source fingerprint,
- no ready snapshot for configured reads.

Each test should assert an error containing `source-metadata-invalid` or the existing explicit trust diagnostic.

- [ ] **Step 5: Run focused source sync tests**

Run:

```bash
cargo test -p crosspack-registry update_filesystem_source_accepts_signed_package_poison -- --test-threads=1
cargo test -p crosspack-registry source_metadata -- --test-threads=1
```

Expected: signed package poison readiness test passes and source-level trust failures still fail closed.

- [ ] **Step 6: Commit**

```bash
git add crates/crosspack-registry/src/source_sync.rs crates/crosspack-registry/src/tests.rs
git commit -m "fix(registry): allow signed package poison in ready snapshots"
```

---

## Task 7: CLI Warning Output For Skipped Package Records

**Files:**
- Modify: `crates/crosspack-cli/src/metadata.rs`
- Modify: `crates/crosspack-cli/src/main.rs` only if the command arm needs to print warnings from the returned outcome
- Modify: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Write failing metadata test**

Add this test near `run_search_command_formats_exact_prefix_and_keyword_matches_deterministically` in `crates/crosspack-cli/src/tests.rs`:

```rust
#[test]
fn run_search_command_returns_skipped_package_warnings() {
    let layout = test_layout();
    configure_ready_source(&layout, "official");
    write_signed_test_manifest(
        &layout,
        "official",
        "good",
        "1.0.0",
        Some("MIT"),
        None,
        &[],
    );

    let cache_root = registry_state_root(&layout).join("cache").join("official");
    let signing_key = test_signing_key();
    let package_template_path = cache_root.join("packages").join("bad.toml");
    let bad_template = "name = [\"bad\"\n";
    std::fs::write(&package_template_path, bad_template.as_bytes())
        .expect("must write malformed package template");
    let package_signature = signing_key.sign(bad_template.as_bytes());
    std::fs::write(
        package_template_path.with_extension("toml.sig"),
        hex::encode(package_signature.to_bytes()),
    )
    .expect("must write malformed package signature");
    let bad_dir = cache_root.join("releases").join("bad");
    std::fs::create_dir_all(&bad_dir).expect("must create bad release dir");
    let release = "version = \"1.0.0\"\n";
    let release_path = bad_dir.join("1.0.0.toml");
    std::fs::write(&release_path, release.as_bytes()).expect("must write bad release");
    let release_signature = signing_key.sign(release.as_bytes());
    std::fs::write(
        release_path.with_extension("toml.sig"),
        hex::encode(release_signature.to_bytes()),
    )
    .expect("must write bad release signature");

    let backend = select_metadata_backend(None, &layout).expect("configured backend must load");
    let outcome = run_search_command(&backend, "").expect("search must succeed");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].name, "good");
    assert_eq!(outcome.skipped_packages.len(), 1);
    assert_eq!(outcome.skipped_packages[0].package, "bad");
    assert_eq!(outcome.skipped_packages[0].reason_code, "package-metadata-invalid");

    let _ = std::fs::remove_dir_all(layout.prefix());
}
```

- [ ] **Step 2: Run test and confirm failure**

Run:

```bash
cargo test -p crosspack-cli run_search_command_returns_skipped_package_warnings -- --test-threads=1
```

Expected: failure because `run_search_command` still returns `Vec<SearchResult>` and does not expose skipped package diagnostics.

- [ ] **Step 3: Use diagnostic-returning registry API in search flow**

In `crates/crosspack-cli/src/metadata.rs`, add this import near existing imports if needed:

```rust
use crosspack_registry::PackageSkipDiagnostic;
```

Add this result type near `SearchResult`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchCommandOutcome {
    results: Vec<SearchResult>,
    skipped_packages: Vec<PackageSkipDiagnostic>,
}
```

Replace `MetadataBackend::search_names` with:

```rust
    fn search_names_with_diagnostics(
        &self,
        query: &str,
    ) -> Result<(Vec<String>, Vec<PackageSkipDiagnostic>)> {
        match self {
            Self::Legacy(index) => index.search_names_with_diagnostics(query),
            Self::Configured(index) => index.search_names_with_diagnostics(query),
        }
    }
```

Change `run_search_command` signature and first lines to:

```rust
fn run_search_command(backend: &MetadataBackend, query: &str) -> Result<SearchCommandOutcome> {
    let query = query.trim();
    let (names, skipped_packages) = backend
        .search_names_with_diagnostics(query)
        .with_context(|| SEARCH_METADATA_GUIDANCE)?;

    let mut results = Vec::new();
    // Keep the existing result-building loop unchanged.
    // Return SearchCommandOutcome { results, skipped_packages } at the end.
```

At the end of `run_search_command`, replace `Ok(results)` with:

```rust
    Ok(SearchCommandOutcome {
        results,
        skipped_packages,
    })
```

Update existing tests that call `run_search_command` to use `.results` before formatting.

- [ ] **Step 4: Print additive warnings in the search command arm**

In `crates/crosspack-cli/src/dispatch.rs` if the search arm lives there, or in `crates/crosspack-cli/src/main.rs` if the dispatch include keeps the arm inline, find the `Commands::Search` arm. After calling `run_search_command`, print warnings before formatting results:

```rust
for diagnostic in &outcome.skipped_packages {
    eprintln!(
        "warning: registry_package_skipped package={} reason={} detail={:?}",
        diagnostic.package, diagnostic.reason_code, diagnostic.detail
    );
}
let lines = format_search_results_for_style(style, &outcome.results, &query);
```

If the command arm already stores `results`, rename that binding to `outcome` and pass `&outcome.results` to the existing formatter.

- [ ] **Step 5: Keep install of selected bad package strict**

Do not use tolerant APIs for selected package resolution. If the user runs `crosspack install bad`, the direct load should fail with the package-specific parse/signature diagnostic.

- [ ] **Step 6: Run focused CLI test**

Run:

```bash
cargo test -p crosspack-cli run_search_command_returns_skipped_package_warnings -- --test-threads=1
```

Expected: selected test passes and existing transaction line shapes are unchanged.

- [ ] **Step 7: Commit**

```bash
git add crates/crosspack-cli/src/metadata.rs crates/crosspack-cli/src/dispatch.rs crates/crosspack-cli/src/main.rs crates/crosspack-cli/src/tests.rs
git commit -m "fix(cli): warn when registry search skips invalid packages"
```

---

## Task 8: Documentation And Runbook

**Files:**
- Modify: `registry/scripts/registry-update-runbook.md`
- Modify: `README.md`
- Modify: `docs/registry-spec.md`
- Modify: `.agents/specs/registry-automation-maturation-spec.md` only if implementation changes the approved design

- [ ] **Step 1: Update registry runbook**

Add a section to `registry/scripts/registry-update-runbook.md`:

```markdown
## Rolling Upstream Release Bot

The scheduled upstream release bot maintains one rolling PR from `upstream-release/rolling`.
Each run starts from current `main`, regenerates valid package updates, writes `state/upstream-release-bot.json`, force-updates the bot-owned branch with `--force-with-lease`, and enables automerge.

Malformed package updates are recorded under `quarantine` in `state/upstream-release-bot.json` and omitted from generated metadata until a later run regenerates valid metadata for that package. Rate-limited packages use per-package/source `backoff_until`; unrelated packages continue in the same run.

Bot PRs may contain unsigned `packages/*.toml` and `releases/*/*.toml` files. The `sign-manifests-on-merge` workflow signs changed sidecars after merge.
```

- [ ] **Step 2: Update root README trust language**

In `README.md`, keep the existing source-level fail-closed text and add:

```markdown
Package-level malformed records are isolated where commands can proceed without selecting that package. Signed malformed package records do not prevent a source snapshot from becoming ready, but source-level trust failures still fail closed: missing or invalid registry keys, bad configured-source fingerprints, missing ready snapshots, and missing or invalid metadata signatures remain fatal.
```

- [ ] **Step 3: Update registry spec**

In `docs/registry-spec.md`, add a package quarantine subsection that states:

```markdown
Automation quarantine is advisory registry state stored under `state/upstream-release-bot.json`. It prevents repeated generated poison from blocking unrelated package updates. Source sync may accept ready snapshots that contain signed malformed package-level records, because signatures prove provenance even when package content is unusable. Clients may skip quarantined or malformed package-level records during broad list/search/provider operations, but they must still fail selected package operations when the selected package metadata is invalid.
```

- [ ] **Step 4: Run doc/spec grep check**

Run:

```bash
rg -n "quarantine|rolling|fail closed|package-level" README.md docs/registry-spec.md registry/scripts/registry-update-runbook.md .agents/specs/registry-automation-maturation-spec.md
```

Expected: each modified document includes the new behavior and preserves fail-closed source trust wording.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/registry-spec.md .agents/specs/registry-automation-maturation-spec.md registry/scripts/registry-update-runbook.md
git commit -m "docs: describe autonomous registry quarantine flow"
```

---

## Task 9: Final Verification

**Files:**
- No source edits expected unless verification fails.

- [ ] **Step 1: Run registry Python tests**

Run from `registry/`:

```bash
python3 -m unittest tests.test_upstream_release_bot tests.test_github_workflows tests.test_registry_validate_source tests.test_sign_changed_manifests
```

Expected: all tests pass.

- [ ] **Step 2: Run root registry tests**

Run from workspace root:

```bash
cargo test -p crosspack-registry -- --test-threads=1
```

Expected: all tests pass.

- [ ] **Step 3: Run CLI tests touched by warnings**

Run:

```bash
cargo test -p crosspack-cli -- --test-threads=1
```

Expected: all tests pass.

- [ ] **Step 4: Run snapshot-flow validation**

Run:

```bash
scripts/validate-snapshot-flow.sh
```

Expected: all snapshot-flow checks pass.

- [ ] **Step 5: Run formatting and lint gate**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: formatting check passes and clippy emits no warnings.

- [ ] **Step 6: Run full build and tests**

Run:

```bash
cargo build --workspace --locked
cargo test --workspace
```

Expected: workspace builds and all tests pass.

- [ ] **Step 7: Inspect git status**

Run:

```bash
git status --short
git -C registry status --short
```

Expected: only intended committed or ready-to-commit changes are present in the root repo and registry submodule.

---

## Implementation Notes

- Do not add signing secrets to scheduled bot runs.
- Do not weaken `registry.pub` fingerprint verification, ready snapshot checks, or selected metadata signature verification.
- Do not make package-level tolerant reads hide direct selected package failures.
- Use `--force-with-lease` only for the bot-owned rolling branch.
- Keep root submodule-only PRs non-release changes.
- Keep generated bot paths limited to `packages/*.toml`, `releases/*/*.toml`, and `state/upstream-release-bot.json`.
