# Crosspack Contributor Playbook

## Purpose
Use this playbook when landing code changes, handling PRs, and preparing release-cutting actions in Crosspack.

## Contribution licensing
- Unless explicitly stated otherwise, contributions are licensed under `MIT OR Apache-2.0`.

## 1) Local setup

Prerequisites:
- Git, Rust (stable), and Cargo
- Access to platform targets you touch (Linux/macOS/Windows when needed)
- Write access to the Crosspack repo

Clone and align:

```bash

git clone https://github.com/spiritledsoftware/crosspack.git
cd crosspack

# use repo default toolchain if needed
rustup default stable
rustup update
```

Common developer commands:

```bash
# keep tooling aligned with CI behavior
rustup run stable cargo fmt --all --check
rustup run stable cargo clippy --workspace --all-targets --all-features -- -D warnings
rustup run stable cargo test --workspace
```

## 2) Local setup + build/test workflow

Use this for feature work and pre-PR validation:

- `rustup run stable cargo fmt --all --check`
- `rustup run stable cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `rustup run stable cargo test --workspace`
- `rustup run stable cargo build --release` (if release binary output is part of merge validation)

If you only touch docs, at least run:

- `rustup run stable cargo fmt --all --check`
- `rustup run stable cargo test --workspace -- --list`

## 3) PR merge flow

Before push:

1. Keep branch target on top of latest `main`.
2. Run full local checks above.
3. For snapshot-related changes, run:

```bash
scripts/validate-snapshot-flow.sh
```

If that script reports `CRIT`, stop and fix before merge.

Then:
- Push your branch.
- Open PR with clear summary + evidence (test commands + results).
- Request review and wait for approval before merge.

## 4) Merge conflict resolution

Use these commands when a rebase or merge brings conflicts:

```bash
git fetch origin
git switch <your-branch>
git rebase origin/main
# or: git pull --rebase origin main

git status --short --untracked-files=no

git diff --name-only --diff-filter=U
```

Resolve each file, then:

```bash
git add <resolved-file>
git rebase --continue
```

If you need a clean rerun:

```bash
git rebase --abort
# inspect conflicts again and restart from a cleaner point
```

Useful conflict helpers:

```bash
git checkout --ours <path>  # prefer local file version
# or
git checkout --theirs <path>  # prefer incoming version
```

## 5) Launch runbook (one page)

### Preflight (before release/merge)
- Branch is up to date with `origin/main`.
- All required checks pass:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo build --workspace --locked`
  - `cargo test --workspace`
  - `scripts/validate-snapshot-flow.sh` (automated in CI at `.github/workflows/ci.yml`, step `Snapshot-flow validation`)
- If dependency manifests/lockfiles changed, ensure `.github/workflows/dependency-review.yml` is green.
- Docs updated for any behavior or contributor-facing changes.
- For install/resolver/transaction changes, reconcile:
  - `docs/architecture.md`
  - `docs/install-flow.md`
  - `docs/manifest-spec.md`
  and mark non-shipped spec sections explicitly as planned.
- Reviewers have approved the PR.

### Deploy
- Merge after approvals, with merge commit policy required by the repo.
- Confirm cross-platform CI jobs are queued (`ubuntu`, `macOS`, `windows`).
- Ensure `CROSSPACK_BOT_APP_ID` (repository variable) and `CROSSPACK_BOT_APP_PRIVATE_KEY` (repository secret) are configured so release tags trigger downstream workflows.
- Stable release flow:
  1. Let `.github/workflows/release-please.yml` create/update the release PR from `main`.
  2. Confirm release PR includes `Cargo.toml` workspace version bump + `CHANGELOG.md` updates.
  3. Merge release PR to create stable tag `vX.Y.Z` and GitHub release metadata.
  4. Confirm `.github/workflows/release-artifacts.yml` completed for that tag and uploaded all target artifacts + `SHA256SUMS.txt`.
- Prerelease flow:
  1. Push to `release/*` branch.
  2. Confirm `.github/workflows/prerelease-artifacts.yml` creates `vX.Y.Z-rc.N` and uploads artifacts/checksums.

### Post-launch
- Monitor CI dashboards for regression windows (especially snapshot checks).
- Run `scripts/validate-snapshot-flow.sh` before promoting a release candidate.
- Run `scripts/check-snapshot-mismatch-health.sh` to detect repeated `snapshot-id-mismatch` errors.
- Promotion steps:
  1. Validate RC artifacts from **Prerelease Artifacts** are complete and downloadable.
  2. Merge intended changes to `main`.
  3. Let **Release Please** produce/refresh the stable release PR.
  4. Merge stable release PR and confirm stable artifact workflow completion.
- Verify no unexpected snapshot mismatch failures in logs.
- Capture and triage any failed jobs before public rollout.

### Rollback
- Identify the bad change (`MERGE_COMMIT`, `PR_NUMBER`, or release tag).
- Rollback path:
  1. Revert merge or release commit.
  2. Reopen/patch the failing work item quickly.
  3. Re-run checks, then re-merge once stable.
  4. Allow Release Please to cut the corrective follow-up release.
- If `scripts/check-snapshot-mismatch-health.sh` returns `CRIT`, open a launch blocker review and include output and mitigation status.
- Announce blocker status and mitigation steps in the issue/thread.

## 6) SPI-19 follow-through

- Contributor playbook now includes local setup, build/test, merge flow, conflict handling, and launch checklist.
- Include the snapshot post-merge validation in merge and release gates.

### SPI-21 troubleshooting: snapshot mismatch monitoring

Use this runbook when snapshot consistency mismatches repeat during release monitoring:

1. Run `scripts/check-snapshot-mismatch-health.sh` and capture the output in the issue thread.
2. Run `cargo run -p crosspack-cli -- registry list` and confirm enabled sources report a shared `ready:<snapshot-id>`.
3. Run `cargo run -p crosspack-cli -- update` and rerun the health check.
4. If the check is still `CRIT`, open launch blocker review, pause promotion, and assign source owners for mismatch triage.
