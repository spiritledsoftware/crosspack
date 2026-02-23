# Registry Bootstrap Runbook (SPI-26)

This runbook defines support and operator procedures for first-run trust bootstrap of the official Crosspack registry source.

## Official Defaults

- Source name: `core`
- Source kind: `git`
- Source URL: `https://github.com/spiritledsoftware/crosspack-registry.git`
- Fingerprint channel:
  - `docs/trust/core-registry-fingerprint.txt`
  - Matching GitHub Release note entry

Always verify both channels match on `fingerprint_sha256`, `updated_at`, and `key_id` before bootstrap.

## First-Run Bootstrap (User)

1. Read `docs/trust/core-registry-fingerprint.txt`.
2. Confirm the same values appear in the latest corresponding GitHub Release note.
3. Add the trusted source and update snapshots:

```bash
crosspack registry add core https://github.com/spiritledsoftware/crosspack-registry.git --kind git --priority 100 --fingerprint <fingerprint_sha256>
crosspack update
crosspack registry list
```

Expected state: `core` appears with `snapshot=ready:<snapshot-id>`.

## Fingerprint and Key Rotation (Operator Procedure)

1. Prepare new signing keypair and stage new `registry.pub` at planned cutover revision.
2. Compute the new fingerprint from raw `registry.pub` bytes.
3. Update `docs/trust/core-registry-fingerprint.txt` with new `fingerprint_sha256`, `updated_at`, and `key_id`.
4. Publish a GitHub Release note entry with exactly matching values.
5. Announce cutover with user recovery commands.
6. Keep rollback window for the previous key; remove old key after successful migration.

User-facing recovery commands during rotation:

```bash
crosspack registry remove core
crosspack registry add core https://github.com/spiritledsoftware/crosspack-registry.git --kind git --priority 100 --fingerprint <new-fingerprint_sha256>
crosspack update
```

If stale cache is suspected:

```bash
crosspack registry remove core --purge-cache
crosspack registry add core https://github.com/spiritledsoftware/crosspack-registry.git --kind git --priority 100 --fingerprint <new-fingerprint_sha256>
crosspack update
```

## Troubleshooting

### fingerprint mismatch (`source-key-fingerprint-mismatch`)

Symptoms:
- `crosspack update` fails for `core`.
- Error includes mismatch/fingerprint wording.

Actions:
1. Re-check `docs/trust/core-registry-fingerprint.txt` against GitHub Release note.
2. Remove and re-add `core` with the published fingerprint.
3. Retry `crosspack update`.

### source sync failed (`source-sync-failed`)

Symptoms:
- Update output: `core: failed (reason=source-sync-failed)`.

Actions:
1. Verify network access to `https://github.com/spiritledsoftware/crosspack-registry.git`.
2. Retry `crosspack update`.
3. If persistent, remove and re-add source, then retry.

### snapshot missing (`source-snapshot-missing`)

Symptoms:
- Metadata commands fail before install/search/info.
- `registry list` shows `snapshot=none` or error state.

Actions:
1. Run `crosspack update`.
2. If failure remains, inspect update reason and resolve root cause.
3. If needed, run cache purge flow and re-bootstrap `core`.

### metadata invalid (`source-metadata-invalid`)

Symptoms:
- Update or metadata read fails with signature/metadata validation context.

Actions:
1. Confirm registry key and fingerprint channels still match.
2. Retry after fresh sync (`crosspack update`).
3. Escalate to registry operators with failing package path and error text.
