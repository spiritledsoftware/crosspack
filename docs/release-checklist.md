# Release Checklist

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

## Snapshot Mismatch Health Check (SPI-21)

Validate that release telemetry is not showing repeated `snapshot-id-mismatch` errors:

```bash
scripts/check-snapshot-mismatch-health.sh
```

Interpretation:
- `PASS`: no recent mismatch bursts; proceed with normal launch review.
- `WARN`: recent mismatches observed; investigate before promotion.
- `CRIT`: repeated mismatches detected; open a launch blocker review and attach check output before proceeding.
