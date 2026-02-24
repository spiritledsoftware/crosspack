# Release Checklist

## Release Automation Workflows

- Release PR + version/changelog automation: `.github/workflows/release-please.yml`
- Stable release artifacts (`vX.Y.Z`): `.github/workflows/release-artifacts.yml`
- Registry sync from stable release (`vX.Y.Z`): `.github/workflows/registry-sync.yml`
- Prerelease artifacts (`vX.Y.Z-rc.N` from `release/*`): `.github/workflows/prerelease-artifacts.yml`

## Stable Release Flow (`main`)

1. Confirm repository variable `CROSSPACK_BOT_APP_ID` and repository secret `CROSSPACK_BOT_APP_PRIVATE_KEY` are configured for `.github/workflows/release-please.yml`.
2. Confirm registry sync configuration for `.github/workflows/registry-sync.yml`:
   - repository variable `CROSSPACK_REGISTRY_REPOSITORY` (default `spiritledsoftware/crosspack-registry`) or `CROSSPACK_REGISTRY_REPOSITORY_NAME`
   - repository secret `CROSSPACK_REGISTRY_SIGNING_PRIVATE_KEY_PEM` (Ed25519 private key PEM used to sign `index/crosspack/<version>.toml`)
   - GitHub App installation has `contents:write` on the registry repository
3. Verify merged commits on `main` follow Conventional Commits:
   - `fix:` -> patch bump
   - `feat:` -> minor bump
   - `BREAKING CHANGE:` footer -> major bump
4. Confirm **Release Please** has an open release PR with:
   - `Cargo.toml` workspace version bump
   - `CHANGELOG.md` updates
5. Validate CI checks on release PR:
   - `cargo fmt --all --check`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo build --workspace --locked`
   - `cargo test --workspace`
6. Merge the release PR.
7. Confirm stable tag `vX.Y.Z` and GitHub release were created.
8. Confirm **Release Artifacts** completed and uploaded:
   - `crosspack-<release_tag>-x86_64-unknown-linux-gnu.tar.gz`
   - `crosspack-<release_tag>-aarch64-unknown-linux-gnu.tar.gz`
   - `crosspack-<release_tag>-x86_64-unknown-linux-musl.tar.gz`
   - `crosspack-<release_tag>-aarch64-unknown-linux-musl.tar.gz`
   - `crosspack-<release_tag>-x86_64-apple-darwin.tar.gz`
   - `crosspack-<release_tag>-aarch64-apple-darwin.tar.gz`
   - `crosspack-<release_tag>-x86_64-pc-windows-msvc.zip`
   - `SHA256SUMS.txt`

## Prerelease Flow (`release/*`)

1. Create or update a `release/*` branch from the candidate commit.
2. Ensure `Cargo.toml` `[workspace.package].version` matches the intended base version.
3. Push to `release/*` and confirm **Prerelease Artifacts** runs automatically.
4. Verify generated prerelease tag format: `v<version>-rc.<run_number>`.
5. Verify prerelease assets and checksums are attached to the GitHub prerelease.

## Manual Override and Rollback

- Rebuild or republish a specific stable/prerelease tag via manual dispatch of `.github/workflows/release-artifacts.yml` with `release_tag`.
- For a bad stable release:
  1. Revert the release commit on `main`.
  2. Merge fix PR(s).
  3. Let Release Please cut a corrective follow-up release.
- For a bad prerelease:
  1. Fix on `release/*`.
  2. Push again to generate the next `-rc.N` tag.
  3. Mark superseded prerelease entries clearly in release notes.

## Post-Merge Snapshot Validation (SPI-20)

Automated enforcement location: `.github/workflows/ci.yml` (`Snapshot-flow validation` step in job `test`) runs `scripts/validate-snapshot-flow.sh` on push and pull_request events for non-doc changes.

For release gating, still run focused snapshot-flow checks after merge and before release packaging:

```bash
scripts/validate-snapshot-flow.sh
```

Interpretation:
- `PASS`: all snapshot-flow hardening checks passed.
- `WARN`: validation passed but environment or speed hints were raised; review and address when practical.
- `CRIT`: one or more snapshot consistency checks failed; release must not proceed until fixed.

## Docs-Claim Verification Pass (SPI-23)

Before release promotion, run a docs-claim pass to ensure launch-facing wording only promises shipped GA behavior:

- Confirm README and architecture docs describe current shipped scope.
- Confirm v0.4/v0.5 specs are labeled as roadmap drafts (non-GA).
- Confirm command examples/help text do not imply unimplemented guarantees.
- Reconcile this docs set before promotion:
  - `docs/architecture.md`
  - `docs/install-flow.md`
  - `docs/manifest-spec.md`

If any claim is ambiguous, fix docs before continuing release promotion.

## Snapshot Mismatch Health Check (SPI-21)

Validate that release telemetry is not showing repeated `snapshot-id-mismatch` errors:

```bash
scripts/check-snapshot-mismatch-health.sh
```

Interpretation:
- `PASS`: no recent mismatch bursts; proceed with normal launch review.
- `WARN`: recent mismatches observed; investigate before promotion.
- `CRIT`: repeated mismatches detected; open a launch blocker review and attach check output before proceeding.
