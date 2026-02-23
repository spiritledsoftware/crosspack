# Crosspack Contributor Playbook

## Purpose
Use this playbook when landing code changes, handling PRs, and preparing release-cutting actions in Crosspack.

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
  - `cargo test --workspace`
  - `scripts/validate-snapshot-flow.sh` (automated in CI at `.github/workflows/ci.yml`, step `Snapshot-flow validation`)
- Docs updated for any behavior or contributor-facing changes.
- Reviewers have approved the PR.

### Deploy
- Merge after approvals, with merge commit policy required by the repo.
- Confirm cross-platform CI jobs are queued (`ubuntu`, `macOS`, `windows`).
- Confirm release artifacts are generated with the repo's expected target flags.

### Post-launch
- Monitor CI dashboards for regression windows (especially snapshot checks).
- Run `scripts/validate-snapshot-flow.sh` before promoting a release candidate.
- Verify no unexpected snapshot mismatch failures in logs.
- Capture and triage any failed jobs before public rollout.

### Rollback
- Identify the bad change (`MERGE_COMMIT`, `PR_NUMBER`, or release tag).
- Rollback path:
  1. Revert merge or release commit.
  2. Reopen/patch the failing work item quickly.
  3. Re-run checks, then re-merge once stable.
- Announce blocker status and mitigation steps in the issue/thread.

## 6) SPI-19 follow-through

- Contributor playbook now includes local setup, build/test, merge flow, conflict handling, and launch checklist.
- Include the snapshot post-merge validation in merge and release gates.
