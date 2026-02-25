# Cargo Lock Refresh in Release Workflow Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refresh `Cargo.lock` during each `main`-branch release cycle so release automation uses current dependency resolution.

**Architecture:** Extend `.github/workflows/release-please.yml` with a pre-release lockfile refresh stage. The job updates lockfile state, validates it with locked resolution, conditionally commits only `Cargo.lock`, then runs `release-please` with existing app-token permissions. Loop avoidance is handled by commit-message and actor guards.

**Tech Stack:** GitHub Actions YAML, Rust/Cargo (`cargo update`, `cargo check --locked`), git CLI in runner.

---

### Task 1: Add lockfile refresh and safety guards in release workflow

**Files:**
- Modify: `.github/workflows/release-please.yml`

**Step 1: Write the failing test (policy check for missing refresh step)**

Create a temporary local policy assertion by checking that the workflow currently does not contain a `Refresh Cargo.lock` step.

Run: `rg "Refresh Cargo.lock" .github/workflows/release-please.yml`
Expected: no matches (acts as failing precondition).

**Step 2: Run precondition command to verify current gap**

Run: `rg "cargo check --workspace --locked" .github/workflows/release-please.yml`
Expected: no matches.

**Step 3: Write minimal implementation in workflow**

Add these logical workflow updates:
- Checkout step before any git/cargo operations.
- Rust toolchain setup step (`dtolnay/rust-toolchain@stable`).
- Lockfile refresh step that updates dependencies for release workflow.
- Validation step: `cargo check --workspace --locked`.
- Change-detection step setting output (e.g., `lock_changed=true/false`) by checking `git diff --quiet -- Cargo.lock`.
- Conditional commit step (only when lock changed) that:
  - configures bot identity,
  - stages only `Cargo.lock`,
  - commits with a deterministic message such as `chore(lockfile): refresh Cargo.lock for release`,
  - pushes to `main`.
- Guard `Run release-please` step so lockfile-only self-commit does not recurse (for example, skip when head commit message matches lockfile refresh commit marker).

**Step 4: Run local verification for workflow content**

Run: `rg "Refresh Cargo.lock|cargo check --workspace --locked|chore\(lockfile\): refresh Cargo.lock for release" .github/workflows/release-please.yml`
Expected: all patterns matched.

**Step 5: Commit**

```bash
git add .github/workflows/release-please.yml
git commit -m "ci: refresh Cargo.lock during release workflow"
```

### Task 2: Validate behavior for no-op and update paths

**Files:**
- Modify: `.github/workflows/release-please.yml` (if fixes needed)

**Step 1: Write the failing test (simulate expected guards not present)**

Check that skip/guard condition is explicitly encoded in workflow for self-commit loop prevention.

Run: `rg "if: .*lockfile.*|if: .*steps\..*lock_changed" .github/workflows/release-please.yml`
Expected: at least one match after implementation; if none, treat as failing test.

**Step 2: Run lint/parse check for YAML validity**

Run: `python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release-please.yml')); print('ok')"`
Expected: `ok`.

**Step 3: Write minimal implementation fixes (if needed)**

If guard or syntax issues appear:
- fix conditional expressions,
- ensure step IDs referenced in conditions exist,
- ensure shell snippets use `set -euo pipefail` where appropriate.

**Step 4: Re-run validation commands**

Run: `python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release-please.yml')); print('ok')" && rg "cargo check --workspace --locked|lock_changed|refresh Cargo.lock" .github/workflows/release-please.yml`
Expected: YAML parses and all required markers are present.

**Step 5: Commit**

```bash
git add .github/workflows/release-please.yml
git commit -m "ci: guard release workflow lockfile refresh"
```
