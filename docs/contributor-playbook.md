# Contributor Playbook

## SPI-19 Snapshot Workflow Follow-Through

After merge (and again before release), run snapshot-flow validation introduced by SPI-20:

```bash
scripts/validate-snapshot-flow.sh
```

If the script reports `CRIT`, fix the failing check first. The script exits non-zero on `CRIT`, so it can be used in local PR/release validation gates.
