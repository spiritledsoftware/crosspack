# Release Checklist

## Release Artifacts Workflow (SPI-22)

Workflow: `.github/workflows/release-artifacts.yml` (GitHub Actions: `https://github.com/spiritledsoftware/crosspack/actions/workflows/release-artifacts.yml`)

1. Choose release trigger:
   - Final release: push `v*` tag (example `v0.2.0`).
   - RC build: run **Release Artifacts** with manual dispatch and set `release_tag` (example `v0.2.0-rc.1`).
2. Verify all target artifacts are uploaded:
   - `crosspack-<release_tag>-x86_64-unknown-linux-gnu.tar.gz`
   - `crosspack-<release_tag>-x86_64-apple-darwin.tar.gz`
   - `crosspack-<release_tag>-x86_64-pc-windows-msvc.zip`
3. Verify uploaded artifact records use name `crosspack-<release_tag>-<target>` and 30-day retention.
4. Promotion from RC to final:
   - Ensure RC artifact set is complete.
   - Create/move final `v*` tag to promoted commit.
   - Wait for tag-triggered workflow to complete.
   - Attach workflow run URL and checksums to release notes.

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

- Confirm README and architecture docs describe current v0.3 shipped scope.
- Confirm v0.4/v0.5 specs are labeled as roadmap drafts (non-GA).
- Confirm command examples/help text do not imply unimplemented guarantees.

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
