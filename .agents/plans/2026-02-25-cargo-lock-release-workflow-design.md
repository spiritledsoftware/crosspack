## Goal

Ensure each new release cycle refreshes `Cargo.lock` with the latest package versions used by the crosspack workspace crates, integrated directly into the GitHub release workflow.

## Chosen Approach

Implement lockfile refresh inside `.github/workflows/release-please.yml` (Release Please workflow), before running the release-please action.

This keeps the process tied to release automation and avoids separate operational workflows.

## Alternatives Considered

1. Dedicated scheduled/manual lockfile-refresh PR workflow.
   - Pros: clear separation from release logic.
   - Cons: not guaranteed to run per release; potential drift.
2. Post-process release PR branch to inject lockfile updates.
   - Pros: lockfile lives directly inside release PR branch.
   - Cons: branch/token orchestration complexity.

## Workflow Architecture

- File: `.github/workflows/release-please.yml`
- Add steps to:
  - set up Rust,
  - refresh lockfile,
  - detect whether `Cargo.lock` changed,
  - conditionally commit only `Cargo.lock` with bot credentials,
  - continue to `googleapis/release-please-action@v4`.

## Data Flow

1. Workflow triggers on push to `main`.
2. Lockfile update command runs (targeted to crosspack crates, or workspace-wide if explicitly preferred).
3. If `Cargo.lock` is unchanged: skip commit path.
4. If changed: create a bot commit containing only `Cargo.lock`.
5. Run release-please so release artifacts and PR metadata reflect current lockfile state.

## Error Handling and Safety

- Fail fast if lockfile update command errors.
- Stage/commit only `Cargo.lock` to avoid accidental file inclusion.
- Reuse existing GitHub App token permissions model for write operations.
- Add loop-avoidance guard logic so bot-generated lockfile commits do not create infinite workflow runs.

## Verification Strategy

- After refresh, run `cargo check --workspace --locked`.
- Validate three scenarios:
  1. no-op (no lockfile delta),
  2. lockfile changed and committed,
  3. no trigger loop from bot commit.

## Expected Outcome

Release cycles consistently include an up-to-date `Cargo.lock`, reducing stale dependency metadata during release preparation while keeping release automation centralized.
