# CSIF-Agent v1.0.1 Ship-Ready Documentation

This document is the release handoff for v1.0.1 hardening.

It consolidates:

- what changed
- how to configure it
- exact API contracts
- validation commands used before ship

Primary troubleshooting reference: [DIAGNOSTICS.md](DIAGNOSTICS.md).

## Scope

This release adds and validates:

- modular lobe admin observability and reload control
- optional admin endpoint authentication guard
- elaborated describe responses for `What is ...?`
- crystallized (data-driven) describe response templates in `grammar.toml`

## Quick Start (Runtime)

```bash
CSIF_BANK_PATH=./my_brain.rwif \
CSIF_GRAMMAR_PATH=./grammar.toml \
CSIF_LOBES_DIR=./lobes \
CSIF_LOBES_POLL_SECS=5 \
CSIF_PORT=8080 \
./target/release/agent_demo
```

To enable admin auth:

```bash
export CSIF_ADMIN_TOKEN="your-shared-token"
```

## Environment Variables

| Variable | Default | Purpose |
|---|---|---|
| `CSIF_BANK_PATH` | `./my_brain.rwif` | RWIF bank path |
| `CSIF_GRAMMAR_PATH` | `./grammar.toml` | Grammar + template config |
| `CSIF_PORT` | `8080` | HTTP listen port |
| `CSIF_LOBES_DIR` | unset | Lobe bundle root directory |
| `CSIF_LOBES_POLL_SECS` | `5` | Lobe refresh polling interval (`0` disables polling) |
| `CSIF_LOBES_STRICT` | `0` | Strict mode for lobe loading |
| `CSIF_ADMIN_TOKEN` | unset | Optional auth guard for `/admin/lobes*` endpoints |

## Admin API Contracts

### GET /admin/lobes

Returns configured lobe runtime settings and currently applied bundles.

Example:

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

### POST /admin/lobes/reload

Forces one refresh pass from `CSIF_LOBES_DIR` and returns the refresh report + applied set.

Example:

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

### Admin Auth Behavior

If `CSIF_ADMIN_TOKEN` is unset:

- `/admin/lobes` and `/admin/lobes/reload` are open.

If `CSIF_ADMIN_TOKEN` is set:

- requests must include either:
  - `X-CSIF-Admin-Token: <token>`
  - `Authorization: Bearer <token>`
- otherwise response is HTTP `401`.

Auth examples:

```bash
curl -s -H "X-CSIF-Admin-Token: $CSIF_ADMIN_TOKEN" http://localhost:8080/admin/lobes
curl -s -X POST -H "Authorization: Bearer $CSIF_ADMIN_TOKEN" http://localhost:8080/admin/lobes/reload
```

## Describe Response Behavior

`What is ...?` now produces elaborated, deterministic responses from explicit graph facts.

Pattern:

1. Direct classification from `is_a` edges.
2. Optional properties phrase from `has_property` edges.
3. Optional subtype examples from reverse `is_a` edges.

Example output:

```text
[CRYSTAL] A bird is an animal. It can be warm-blooded and aquatic. There are several types, including robin and penguin.
```

## Crystallized Templates in grammar.toml

Describe language is configured under `[templates.describe]`.

```toml
[templates.describe]
classification = "A {subject} is {direct}."
properties_intro = "It can be"
properties_outro = "."
property_connector = "and"
subtypes_intro = "There are several types, including"
subtypes_outro = "."
subtype_connector = "and"
oxford_comma = true
max_subtype_examples = 5
```

Fields:

- `classification`: sentence template supporting placeholders `{subject}` and `{direct}`.
- `properties_intro`: prefix phrase before property list.
- `properties_outro`: trailing punctuation/string after property list.
- `property_connector`: list connector for properties.
- `subtypes_intro`: prefix phrase before subtype list.
- `subtypes_outro`: trailing punctuation/string after subtype list.
- `subtype_connector`: list connector for subtype list.
- `oxford_comma`: whether 3+ item lists use Oxford comma.
- `max_subtype_examples`: max subtype examples in response.

## Ship Validation Commands

Compile check:

```bash
cargo check -p csif-agent -p agent_demo
```

Default behavior smoke (no admin token):

```bash
curl -s http://localhost:8080/admin/lobes
curl -s -X POST http://localhost:8080/admin/lobes/reload
```

Admin auth smoke:

```bash
CSIF_ADMIN_TOKEN=test-secret ./target/release/agent_demo
# In another shell:
curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8080/admin/lobes
curl -s -o /dev/null -w '%{http_code}\n' -H "X-CSIF-Admin-Token: test-secret" http://localhost:8080/admin/lobes
```

Elaboration smoke:

```bash
curl -s -X POST http://localhost:8080/teach -H "Content-Type: application/json" -d '{"text":"A bird is an animal."}'
curl -s -X POST http://localhost:8080/teach -H "Content-Type: application/json" -d '{"text":"A bird has warm-blooded."}'
curl -s -X POST http://localhost:8080/teach -H "Content-Type: application/json" -d '{"text":"A robin is a bird."}'
curl -s -X POST http://localhost:8080/query -H "Content-Type: application/json" -d '{"text":"What is a bird?"}'
```

Template override smoke:

1. Copy `grammar.toml`.
2. Change `properties_intro` or connectors.
3. Restart server with `CSIF_GRAMMAR_PATH` pointing to the modified file.
4. Repeat describe query and verify wording changes without any code edit.

## Release Checklist

- [x] Admin endpoints implemented and documented.
- [x] Optional admin auth guard implemented and documented.
- [x] Elaborated describe responses implemented and documented.
- [x] Describe wording crystallized into grammar templates.
- [x] Compile + runtime smoke checks completed.
- [x] User-facing docs linked from README.
