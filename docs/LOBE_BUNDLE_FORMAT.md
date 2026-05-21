# Lobe Bundle Format and Directory Convention

This document defines the exact bundle format for modular lobes (for example Medical, Legal, Finance) and how CSIF-Agent discovers and applies them.

## Goal

A user can download a lobe bundle, place it in a known directory, and the running agent will discover and apply it automatically (or on next startup), without modifying core code.

## Directory Convention

Default host-side convention:

```text
./lobes/
  medical/
    lobe.toml
    seed/
      taxonomy.txt
      properties.txt
      causality.txt
```

Versioned convention (also supported):

```text
./lobes/
  medical/
    1.0.0/
      lobe.toml
      seed/
        taxonomy.txt
        properties.txt
        causality.txt
```

The loader scans one and two levels under `CSIF_LOBES_DIR` and loads any folder containing `lobe.toml`.

## Environment Variables

| Variable | Default | Meaning |
|---|---|---|
| `CSIF_LOBES_DIR` | unset | Root directory containing lobe bundles. |
| `CSIF_LOBES_POLL_SECS` | `5` | Poll interval for auto-discovery while running (`0` disables polling). |
| `CSIF_LOBES_STRICT` | `0` | If `1`, fail startup/refresh on lobe errors; if `0`, skip invalid bundles. |

## Manifest Format (`lobe.toml`)

```toml
id = "medical"
version = "1.0.0"
compatible_agent = ">=0.1.0, <1.0.0"
priority = 50
enabled = true

seed_files = [
  "seed/taxonomy.txt",
  "seed/properties.txt",
  "seed/causality.txt",
]

[checksum_sha256]
"seed/taxonomy.txt" = "<hex sha256>"
"seed/properties.txt" = "<hex sha256>"
"seed/causality.txt" = "<hex sha256>"
```

### Required fields

- `id` (string): stable lobe identifier, for example `medical`.
- `version` (string): lobe version.
- `seed_files` (string array): relative file paths inside the bundle.

### Optional fields

- `compatible_agent` (string): SemVer requirement for the agent, for example `">=0.1.0, <1.0.0"`.
- `priority` (int): lower value loads first (default: `100`).
- `enabled` (bool): if `false`, lobe is discovered but not applied.
- `checksum_sha256` (table): optional per-file SHA-256 verification.

## Apply Semantics

- Files are processed in deterministic order.
- Facts are parsed via existing grammar and ingested using seed fast-path.
- Empty lines are ignored.
- Invalid lines are counted as ignored (not fatal unless strict mode is enabled).
- If checksums are provided, mismatches fail the bundle.
- Compatibility checks use `compatible_agent` when set.

## Idempotency and State

CSIF-Agent stores applied lobe state in a bank-adjacent file:

```text
<bank>.lobes.json
```

State tracks:

- lobe `id`
- lobe `version`
- lobe `fingerprint` (manifest + seed content hash)

A lobe is applied once per unique `(id, version, fingerprint)`. Restarting the agent does not reapply unchanged lobes.

## Docker Example

```yaml
services:
  csif-agent:
    environment:
      - CSIF_BANK_PATH=/data/my_brain.rwif
      - CSIF_LOBES_DIR=/data/lobes
      - CSIF_LOBES_POLL_SECS=5
    volumes:
      - csif-agent-data:/data
      - ./lobes:/data/lobes:ro
```

Drop a new bundle under `./lobes` and the agent will pick it up on the next poll tick.

## Admin Endpoints

If `CSIF_ADMIN_TOKEN` is set, both admin endpoints require a matching token provided as `X-CSIF-Admin-Token: <token>` or `Authorization: Bearer <token>`.

With the server running, you can inspect and manually refresh lobe state:

```bash
# List currently applied bundles
curl -s http://localhost:8080/admin/lobes

# Trigger on-demand reload from CSIF_LOBES_DIR
curl -s -X POST http://localhost:8080/admin/lobes/reload

# Authenticated variants (when CSIF_ADMIN_TOKEN is set)
curl -s -H "X-CSIF-Admin-Token: $CSIF_ADMIN_TOKEN" http://localhost:8080/admin/lobes
curl -s -X POST -H "Authorization: Bearer $CSIF_ADMIN_TOKEN" http://localhost:8080/admin/lobes/reload
```

Example list response:

```json
{
  "lobe_dir": "./lobes",
  "poll_secs": 5,
  "strict_mode": false,
  "applied": [
    {
      "id": "medical",
      "version": "1.0.0",
      "fingerprint": "eadb5af4a7252ba9884bf64066ccf06efe2bc96fd2b6689f6b8dc3373235bca4"
    }
  ]
}
```

Example reload response:

```json
{
  "lobe_dir": "./lobes",
  "report": {
    "discovered": 1,
    "applied": 0,
    "skipped": 1,
    "taught": 0,
    "ignored": 0
  },
  "applied": [
    {
      "id": "medical",
      "version": "1.0.0",
      "fingerprint": "eadb5af4a7252ba9884bf64066ccf06efe2bc96fd2b6689f6b8dc3373235bca4"
    }
  ]
}
```

## Recommended Publishing Pattern

- Publish each lobe as a separate GitHub Release asset (zip/tar).
- Include `lobe.toml`, seed files, and checksums.
- Version lobes independently from the core agent.
- Keep Base Lobe as default gold bundle.
