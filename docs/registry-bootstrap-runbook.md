# Registry Bootstrap Runbook (SPI-26)

This runbook defines support and operator procedures for first-run trust bootstrap of the official Crosspack registry source.

## Official Defaults

- Source name: `core`
- Source kind: `git`
- Source URL: `https://github.com/spiritledsoftware/crosspack-registry.git`
- Fingerprint source: SHA-256 digest of `registry.pub` from `https://github.com/spiritledsoftware/crosspack-registry`

Always verify fingerprint derivation from trusted `registry.pub` bytes before bootstrap.

## First-Run Bootstrap (User)

1. Derive SHA-256 from trusted `registry.pub` bytes.
2. Add the trusted source and update snapshots:

```bash
crosspack registry add core https://github.com/spiritledsoftware/crosspack-registry.git --kind git --priority 100 --fingerprint <fingerprint_sha256>
crosspack update
crosspack registry list
```

Expected state: `core` appears with `snapshot=ready:<snapshot-id>`.

## Installer Behavior

- `scripts/install.sh` and `scripts/install.ps1` fetch `registry.pub` from `https://github.com/spiritledsoftware/crosspack-registry` at install time and compute its SHA-256 fingerprint for `registry add`.
- Installers fail closed on fetch/hash/validation errors.
- Override only when needed for controlled/offline operations:
  - Unix: `CROSSPACK_CORE_FINGERPRINT=<64-hex>`
  - Windows: `-CoreFingerprint <64-hex>`

## Fingerprint and Key Rotation (Operator Procedure)

1. Prepare new signing keypair and stage new `registry.pub` at planned cutover revision.
2. Compute the new fingerprint from raw `registry.pub` bytes.
3. Publish cutover communication and user recovery commands.
4. Keep rollback window for the previous key; remove old key after successful migration.

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
1. Fetch trusted `registry.pub` bytes from the official registry repository.
2. Recompute fingerprint from trusted `registry.pub` bytes and compare with local `sources.toml` value.
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
1. Confirm `registry.pub` and configured `fingerprint_sha256` still match.
2. Retry after fresh sync (`crosspack update`).
3. Escalate to registry operators with failing package path and error text.
