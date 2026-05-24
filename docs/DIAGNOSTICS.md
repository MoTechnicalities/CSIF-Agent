# CSIF-Agent Diagnostics Guide (Ship)

This guide is the operational diagnostics reference for release and post-release support.

It is optimized for:

- fast triage
- deterministic reproduction
- minimal guesswork

## Incident Note: Forced Play Stall (May 24, 2026)

Symptom:

- `GET /admin/play?force=1` could stall while `/health` still reported `ok`.
- Live tracing showed the force path advancing into preview mode and hanging around transitive candidate selection.

Resolution:

- Added `play-trace` timing around the play cycle and preview path.
- Reworked transitive/property candidate selection to use indexed direct-target lookup and streaming best-candidate selection.
- Added a tighter timing split around transitive selector index construction versus edge scanning so the remaining hot segment stays visible.

## 0. Fast Triage Checklist

Run these in order:

```bash
# 1) Confirm you are in the correct repository root
pwd
ls -1 Cargo.toml grammar.toml scripts/ crates/ apps/

# 2) Compile sanity
cargo check -p csif-agent -p agent_demo

# 3) Start server with explicit paths/port
CSIF_BANK_PATH=/tmp/csif_diag_bank.rwif \
CSIF_GRAMMAR_PATH=./grammar.toml \
CSIF_PORT=18080 \
./target/release/agent_demo

# 4) Health/API sanity (new shell)
curl -s http://127.0.0.1:18080/health
curl -s -X POST http://127.0.0.1:18080/query -H "Content-Type: application/json" -d '{"text":"What is a whale?"}'
```

If any step fails, jump to the corresponding section below.

## 1. Known Critical Failure: Exit Code 127

Symptom:

- scripts fail immediately with exit `127`
- commonly seen when invoking `./scripts/qualify_base_lobe_v1.sh`

Most common root cause:

- running from the wrong folder (for example sibling repository root)

Expected working directory for qualification scripts:

```text
.../CSIF-Rust-Trio/CSIF-Agent
```

Verify:

```bash
pwd
test -x ./scripts/qualify_base_lobe_v1.sh && echo OK || echo MISSING
```

If wrong directory, fix:

```bash
cd /home/mogir/Desktop/Mogir_Jason_Rofick/AI-GitHub_projects/CSIF-Rust-Trio/CSIF-Agent
```

## 2. Build/Compile Diagnostics

### 2.1 Compile Failure

```bash
cargo check -p csif-agent -p agent_demo
```

Expected:

- no errors

If failure:

1. read first error location; ignore downstream cascade errors
2. rerun with full context

```bash
cargo check -p csif-agent -p agent_demo -vv
```

### 2.2 Binary Missing

If `./target/release/agent_demo` not found:

```bash
cargo build --release -p agent_demo
```

## 3. Server Startup Diagnostics

### 3.1 Health Endpoint Never Comes Up

Start with explicit env:

```bash
CSIF_BANK_PATH=/tmp/csif_diag_bank.rwif \
CSIF_GRAMMAR_PATH=./grammar.toml \
CSIF_PORT=18080 \
./target/release/agent_demo
```

In another shell:

```bash
for i in {1..20}; do curl -s http://127.0.0.1:18080/health && break; sleep 0.25; done
```

If still down, check port conflict:

```bash
ss -ltnp | rg ':18080'
```

Pick another port:

```bash
CSIF_PORT=18088 ./target/release/agent_demo
```

### 3.2 Server Exits Early

Run foreground first (do not daemonize) to capture panic/errors.

Common causes:

- invalid `CSIF_GRAMMAR_PATH`
- unreadable bank path
- malformed `grammar.toml`

## 4. API Contract Diagnostics

### 4.1 `/query` and `/teach` payload shape

Required JSON field:

```json
{"text":"..."}
```

Bad payload shape will fail.

Correct examples:

```bash
curl -s -X POST http://127.0.0.1:18080/teach -H "Content-Type: application/json" -d '{"text":"A whale is a mammal."}'
curl -s -X POST http://127.0.0.1:18080/query -H "Content-Type: application/json" -d '{"text":"What is a whale?"}'
```

## 5. Admin Endpoint Diagnostics

Endpoints:

- `GET /admin/lobes`
- `POST /admin/lobes/reload`

### 5.1 Unexpected 401

If `CSIF_ADMIN_TOKEN` is set, auth is required.

Send one of:

```bash
curl -s -H "X-CSIF-Admin-Token: $CSIF_ADMIN_TOKEN" http://127.0.0.1:18080/admin/lobes
curl -s -X POST -H "Authorization: Bearer $CSIF_ADMIN_TOKEN" http://127.0.0.1:18080/admin/lobes/reload
```

### 5.2 Admin routes should be open but are not

Check whether token is set in runtime env:

```bash
env | rg '^CSIF_ADMIN_TOKEN='
```

Unset for open mode:

```bash
unset CSIF_ADMIN_TOKEN
```

## 6. Lobe Loader Diagnostics

### 6.1 Bundle not discovered

Check directory + manifest placement:

```bash
find ./lobes -maxdepth 3 -name lobe.toml -print
```

### 6.2 Bundle discovered but skipped

Likely causes:

- `enabled = false`
- `compatible_agent` mismatch
- checksum mismatch
- parse errors in seed lines

Manual reload:

```bash
curl -s -X POST http://127.0.0.1:18080/admin/lobes/reload
```

Inspect `report` fields (`discovered`, `applied`, `skipped`, `ignored`, `taught`).

### 6.3 Verify applied-state persistence

Applied lobe state is stored adjacent to bank:

```text
<bank>.lobes.json
```

## 7. Describe Elaboration / Template Diagnostics

### 7.1 Describe response too terse

Check required facts exist:

- direct class (`is_a`)
- optional properties (`has_property`)
- optional subtype examples (reverse `is_a`)

Seed minimal set:

```bash
curl -s -X POST http://127.0.0.1:18080/teach -H "Content-Type: application/json" -d '{"text":"A bird is an animal."}'
curl -s -X POST http://127.0.0.1:18080/teach -H "Content-Type: application/json" -d '{"text":"A bird has warm-blooded."}'
curl -s -X POST http://127.0.0.1:18080/teach -H "Content-Type: application/json" -d '{"text":"A robin is a bird."}'
curl -s -X POST http://127.0.0.1:18080/query -H "Content-Type: application/json" -d '{"text":"What is a bird?"}'
```

### 7.2 Template changes not reflected

Templates are loaded at startup from `CSIF_GRAMMAR_PATH`.

After editing `grammar.toml`, restart server.

Quick check fields:

- `[templates.describe]`
- `classification`
- `properties_intro`
- `property_connector`
- `subtypes_intro`
- `subtype_connector`
- `oxford_comma`
- `max_subtype_examples`

## 8. Qualification Pipeline Diagnostics

### 8.1 Full qualification command

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

### 8.2 Useful runtime knobs

```bash
# adaptive HTTP seeding mode tuning
SEED_MAX_TIME=180
SEED_RETRIES=2
SEED_FACT_DELAY=0.03

# benchmark transport tuning
BENCHMARK_HTTP_TIMEOUT=20
BENCHMARK_HTTP_RETRIES=2
```

### 8.3 Investigate qualification artifacts

Qualification creates temp workspace:

```text
/tmp/csif-base-lobe-v1-XXXXXX/
```

Inspect:

- `server.log`
- `benchmark_summary.json`
- `base_lobe_v1_bank.rwif`

## 9. Release Pre-Flight Diagnostics (Copy/Paste)

```bash
set -e
cd /home/mogir/Desktop/Mogir_Jason_Rofick/AI-GitHub_projects/CSIF-Rust-Trio/CSIF-Agent
cargo check -p csif-agent -p agent_demo

# optional: targeted API smoke
CSIF_BANK_PATH=/tmp/csif_preflight.rwif CSIF_GRAMMAR_PATH=./grammar.toml CSIF_PORT=18090 ./target/release/agent_demo &
PID=$!
for i in {1..30}; do curl -s http://127.0.0.1:18090/health >/dev/null 2>&1 && break; sleep 0.2; done
curl -s -X POST http://127.0.0.1:18090/teach -H "Content-Type: application/json" -d '{"text":"A whale is a mammal."}'
curl -s -X POST http://127.0.0.1:18090/query -H "Content-Type: application/json" -d '{"text":"What is a whale?"}'
kill $PID
```

## 10. Escalation Bundle (When Reporting a Bug)

Include:

1. command executed (exact)
2. working directory (`pwd`)
3. env vars used (`CSIF_*` only)
4. first failing output line
5. `server.log` tail (last 100 lines)
6. if qualification-related: `benchmark_summary.json`

This keeps bug reports reproducible and shortens fix time.
