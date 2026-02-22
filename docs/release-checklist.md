# Release Checklist

## Post-Merge Snapshot Validation (SPI-20)

Run focused snapshot-flow checks after merge and before release packaging:

```bash
scripts/validate-snapshot-flow.sh
```

Interpretation:
- `PASS`: all snapshot-flow hardening checks passed.
- `WARN`: validation passed but environment or speed hints were raised; review and address when practical.
- `CRIT`: one or more snapshot consistency checks failed; release must not proceed until fixed.
