# Base Lobe v1 Tooling Guide

Complete reference for all Base Lobe v1 scripts, commands, and operational procedures.

---

## Quick Start

```bash
cd /path/to/CSIF-Agent

# Build seed data and benchmark assets
python3 scripts/build_base_lobe_v1_assets.py

# Run a full isolated qualification (seeding + benchmarking)
./scripts/qualify_base_lobe_v1.sh

# Check progress toward quotas
python3 scripts/base_lobe_v1_progress.py
```

---

## Script Reference

### `build_base_lobe_v1_assets.py`

**Purpose:** Generate seed data files and benchmark test cases from programmatic definitions.

**Usage:**
```bash
python3 scripts/build_base_lobe_v1_assets.py
```

**Inputs:** None (uses internal functions).

**Outputs:**
- `data/base_lobe_v1/seed/taxonomy.txt` (129 facts)
- `data/base_lobe_v1/seed/causality.txt` (30 facts)
- `data/base_lobe_v1/seed/properties.txt` (30 facts)
- `data/base_lobe_v1/seed/geography.txt` (63 facts)
- `data/base_lobe_v1/seed/arithmetic.txt` (441 facts)
- `data/base_lobe_v1/seed/operator_utility.txt` (20 facts)
- `data/base_lobe_v1/benchmarks/base_lobe_v1_benchmark.jsonl` (600 test cases)

**Output Format (seeds):**
```
a whale is a mammal
a mammal is an animal
rain causes wet ground
a whale has warm-blooded
```

**Output Format (benchmark JSONL):**
```json
{"id":"q-direct-taxo-0001","type":"query","query":"What is a whale?","expected_mode":"contains","expected":"mammal","category":"taxonomy"}
{"id":"q-trans-0001","type":"query","query":"Is a whale an animal?","expected_mode":"contains","expected":"YES","category":"transitive"}
{"id":"t-contr-0001","type":"teach","teach":"a whale is a mineral","expected_mode":"contains_any","expected_any":["CONTRADICTION","already know"],"category":"contradiction"}
```

**Idempotency:** Safe to run repeatedly; overwrites previous assets.

**Notes:**
- Facts are generated deterministically; output is reproducible across runs.
- Benchmark cases are designed to test categories in order: direct, transitive, properties, causality, arithmetic, contradiction, honesty.

---

### `seed_base_lobe_v1.sh`

**Purpose:** Teach seed facts to a running CSIF-Agent instance via HTTP POST to `/teach` endpoint.

**Usage:**
```bash
AGENT_URL="http://localhost:18080" bash scripts/seed_base_lobe_v1.sh
```

**Environment Variables:**
| Variable | Default | Purpose |
|---|---|---|
| `AGENT_URL` | `http://localhost:18080` | Target agent instance. |
| `SEED_VERBOSE` | `1` | Print each fact teach result (set to `0` for quiet). |
| `SEED_MAX_TIME` | `20` | Per-request timeout in seconds. |
| `SEED_RETRIES` | `5` | Retry attempts per failed teach. |
| `SEED_FACT_DELAY` | `0.02` | Sleep between each fact (seconds), prevents overwhelming server. |

**Outputs:**
```
Seeding: taxonomy.txt
  a whale is a mammal
  -> [TEACHING] Knowledge crystallized.
  ...
Seed summary: taught=272 failed=0
Base lobe v1 seed pass complete.
```

**Exit Codes:**
- `0`: Success (taught all facts).
- `2`: Seeding failed (see error details in output).
- `3`: Another seed process already running (lock held).

**Behavior:**
- Reads seed files from `data/base_lobe_v1/seed/*.txt`.
- Posts each fact to `$AGENT_URL/teach`.
- Retries up to `$SEED_RETRIES` times on timeout.
- Skips arithmetic teach (compute is query-native).
- Logs failures with health check status (is server up?).

**Example (manual seeding):**
```bash
# Seed into local agent on port 18080
AGENT_URL="http://localhost:18080" \
SEED_VERBOSE=1 \
bash scripts/seed_base_lobe_v1.sh

# Expected: "Base lobe v1 seed pass complete." if all facts taught.
```

---

### `run_base_lobe_v1_benchmark.py`

**Purpose:** Execute 600 test cases against a running agent and report pass/fail by category.

**Usage:**
```bash
python3 scripts/run_base_lobe_v1_benchmark.py http://localhost:18080 benchmark_summary.json
```

**Arguments:**
1. `AGENT_URL` (optional, default: `http://localhost:18080`): Target agent.
2. `SUMMARY_JSON` (optional): Path to write JSON scorecard (if provided).

**Environment Variables:**
| Variable | Default | Purpose |
|---|---|---|
| `BENCHMARK_VERBOSE` | `0` | Print each test result (set to `1` for debug). |
| `BENCHMARK_HTTP_TIMEOUT` | `8.0` | Per-request timeout in seconds. |
| `BENCHMARK_HTTP_RETRIES` | `1` | Retry attempts per failed request. |

**Outputs (stdout):**
```
Benchmark summary
  passed: 600
  failed: 0

Category summary
  arithmetic: 60/60 (100.0%)
  causality: 45/45 (100.0%)
  contradiction: 45/45 (100.0%)
  honesty: 45/45 (100.0%)
  properties: 165/165 (100.0%)
  taxonomy: 120/120 (100.0%)
  transitive: 120/120 (100.0%)
```

**Outputs (JSON, if path provided):**
```json
{
  "agent_url": "http://localhost:18080",
  "passed": 600,
  "failed": 0,
  "total": 600,
  "categories": {
    "taxonomy": {"pass": 120, "fail": 0},
    "transitive": {"pass": 120, "fail": 0},
    ...
  }
}
```

**Exit Codes:**
- `0`: All checks passed.
- `1`: One or more checks failed (see category summary).

**Behavior:**
- Reads benchmark cases from `data/base_lobe_v1/benchmarks/base_lobe_v1_benchmark.jsonl`.
- For each test case:
  - Sends `/query` or `/teach` request.
  - Checks response against expected output.
  - Records pass/fail and category.
- Handles transient timeouts with retry logic.
- Does NOT modify bank state (only `/query` and read-only `/teach` assertions).

**Example (manual benchmarking):**
```bash
# Run benchmark, save JSON results
python3 scripts/run_base_lobe_v1_benchmark.py \
  http://localhost:18080 \
  /tmp/results.json

# View results
cat /tmp/results.json | jq '.categories'
```

---

### `qualify_base_lobe_v1.sh`

**Purpose:** Orchestrate a complete isolated qualification: spin up a temporary server, seed the base lobe, run benchmarks, and report pass/fail.

**Usage:**
```bash
./scripts/qualify_base_lobe_v1.sh
```

**Environment Variables:**
| Variable | Default | Purpose |
|---|---|---|
| `BASE_LOBE_PORT` | `18081` | Port for isolated test server. |

**Outputs:**
```
Base Lobe v1 assets generated.
Seed counts: ...
Benchmark composition: ...
Seeding base lobe v1 into isolated bank...
  Seed summary: taught=272 failed=0
Running benchmark...
  Benchmark summary: passed=600 failed=0
Qualification workspace: /tmp/csif-base-lobe-v1-fnliPk
Qualification result: PASS
```

**Exit Codes:**
- `0`: Qualification PASSED (all checks, all gates).
- `1`: Qualification FAILED (benchmark or gates).
- `2`: Qualification FAILED (seeding error).

**Behavior:**
1. Cleans up stale processes from prior runs.
2. Builds seed and benchmark assets.
3. Compiles agent binary (release mode).
4. Spins up isolated agent on `127.0.0.1:$BASE_LOBE_PORT`.
5. Seeds base lobe into temporary bank.
6. Runs 600-check benchmark.
7. Writes JSON scorecard to temp workspace.
8. Reports pass/fail and cleanup path.
9. Exits with appropriate code.

**Artifacts:**
- Temporary workspace at `/tmp/csif-base-lobe-v1-XXXXXX/`:
  - `base_lobe_v1_bank.rwif` (seeded facts).
  - `server.log` (agent output).
  - `benchmark_summary.json` (scorecard).
- Auto-cleaned on exit (trap cleanup).

**Example (full qualification):**
```bash
# Run isolated qualification
./scripts/qualify_base_lobe_v1.sh

# If PASS, results at: /tmp/csif-base-lobe-v1-XXXXXX/benchmark_summary.json
# If FAIL, server log at: /tmp/csif-base-lobe-v1-XXXXXX/server.log
```

---

### `base_lobe_v1_progress.py`

**Purpose:** Display current progress toward quota targets and benchmark composition.

**Usage:**
```bash
python3 scripts/base_lobe_v1_progress.py
```

**Inputs:** None (reads from disk).

**Outputs:**
```
Base Lobe v1 Progress
=====================
taxonomy            129/8000   (  1.6%)
causality            30/2500   (  1.2%)
properties           30/3000   (  1.0%)
geography            63/2000   (  3.1%)
arithmetic          441/1500   ( 29.4%)
operator_utility     20/1000   (  2.0%)
---------------------
total facts          713/18000  (  4.0%)

Benchmark composition
=====================
arithmetic           60
causality            45
contradiction        45
honesty              45
properties          165
taxonomy            120
transitive          120
total checks         600
```

**Idempotency:** Safe to run repeatedly; read-only.

---

## Operational Workflows

### Workflow 1: Develop & Qualify a New Seed Batch

**Goal:** Expand seeds, validate they don't break qualification.

```bash
# Step 1: Edit seed files or modify build_base_lobe_v1_assets.py
# (e.g., add new facts to taxonomy.txt or increase generation quotas)

# Step 2: Regenerate assets
python3 scripts/build_base_lobe_v1_assets.py

# Step 3: Check progress
python3 scripts/base_lobe_v1_progress.py

# Step 4: Run isolated qualification
./scripts/qualify_base_lobe_v1.sh

# Step 5: If PASS, commit; if FAIL, debug using server.log
if [ $? -eq 0 ]; then
  git add data/ docs/ scripts/
  git commit -m "feat: base lobe v1 scaling to N facts"
else
  echo "Qualification failed; check /tmp/csif-base-lobe-v1-*/server.log"
fi
```

### Workflow 2: Seeding into a Live Agent

**Goal:** Teach base lobe facts into a running CSIF-Agent instance for development/demo.

```bash
# Step 1: Start agent on target port (default 18080)
cd /path/to/CSIF-Agent
# (run agent, e.g., ./run_demo.sh or cargo run)

# Step 2: Seed base lobe in separate terminal
AGENT_URL="http://localhost:18080" \
SEED_VERBOSE=1 \
bash scripts/seed_base_lobe_v1.sh

# Step 3: Query agent to verify
curl -X POST http://localhost:18080/query \
  -H "Content-Type: application/json" \
  -d '{"text":"What is a whale?"}'

# Expected response: "[CRYSTAL] A whale is a mammal."
```

### Workflow 3: Manual Benchmark Against Existing Bank

**Goal:** Verify a hand-seeded or modified bank against the 600-check suite.

```bash
# Step 1: Make sure agent is running
# (with desired bank loaded)

# Step 2: Run benchmark
python3 scripts/run_base_lobe_v1_benchmark.py \
  http://localhost:18080 \
  /tmp/my_results.json

# Step 3: Review results
cat /tmp/my_results.json | jq '.'

# If failures, investigate with BENCHMARK_VERBOSE=1
BENCHMARK_VERBOSE=1 python3 scripts/run_base_lobe_v1_benchmark.py \
  http://localhost:18080 | grep FAIL
```

### Workflow 4: Scaling to Next Milestone

**Goal:** Grow seeds from current state to next quota target (e.g., 3K → 8K facts).

```bash
# Step 1: Identify which category is undersized
python3 scripts/base_lobe_v1_progress.py

# Step 2: Edit build_base_lobe_v1_assets.py to increase generation quota
# (e.g., expand build_taxonomy() to yield more hyponym chains)

# Step 3: Regenerate
python3 scripts/build_base_lobe_v1_assets.py

# Step 4: Check new counts
python3 scripts/base_lobe_v1_progress.py

# Step 5: Qualify
./scripts/qualify_base_lobe_v1.sh

# Step 6: If gates pass, commit and tag milestone
git add data/ scripts/
git commit -m "feat: milestone N — base lobe scaling to M facts"
git tag -a v1-milestone-N -m "M facts seeded, all gates pass"
```

---

## Debugging & Troubleshooting

### Issue: "Another seed process already running"

**Cause:** Lock file `/tmp/csif_base_lobe_v1_seed.lock` held by prior process.

**Resolution:**
```bash
# Find and kill stale seed process
pkill -f "seed_base_lobe_v1.sh"

# Remove lock file
rm -f /tmp/csif_base_lobe_v1_seed.lock

# Retry seeding
bash scripts/seed_base_lobe_v1.sh
```

### Issue: Qualification times out during seeding

**Cause:** Agent is slow to respond (high latency, overloaded).

**Resolution:**
```bash
# Increase seed timeout and retries
SEED_MAX_TIME=30 SEED_RETRIES=3 ./scripts/qualify_base_lobe_v1.sh

# Or check agent server log:
tail -n 100 /tmp/csif-base-lobe-v1-XXXXXX/server.log
```

### Issue: Benchmark shows low honesty rate (fabricating answers)

**Cause:** Agent answering unknowns instead of rejecting.

**Resolution:**
```bash
# Debug a specific honesty check:
curl -X POST http://localhost:18080/query \
  -H "Content-Type: application/json" \
  -d '{"text":"What is a unknown_entity_001?"}'

# Should respond with: "[NEEDS_INPUT]..." or similar negative.
# If it invents an answer, the base lobe may have spurious edges.
```

### Issue: Qualification passes but benchmark has wrong category counts

**Cause:** Benchmark generation in `build_base_lobe_v1_assets.py` out of sync.

**Resolution:**
```bash
# Regenerate benchmark
python3 scripts/build_base_lobe_v1_assets.py

# Verify composition
wc -l data/base_lobe_v1/benchmarks/base_lobe_v1_benchmark.jsonl
# Should be 600

# Verify categories
grep '"category"' data/base_lobe_v1/benchmarks/base_lobe_v1_benchmark.jsonl | \
  cut -d'"' -f 8 | sort | uniq -c
# Should show: 60 arithmetic, 45 causality, 45 contradiction, 45 honesty, 165 properties, 120 taxonomy, 120 transitive
```

---

## Maintenance & Iteration

### Quarterly Audit
```bash
# Run full qualification
./scripts/qualify_base_lobe_v1.sh

# Check reproducibility (run 3x, compare JSON scorecards)
for i in {1..3}; do
  ./scripts/qualify_base_lobe_v1.sh 2>&1 | grep "Qualification result"
done

# All should show PASS
```

### Adding a New Category
1. Define programmatic generation in `build_base_lobe_v1_assets.py` (new `build_X()` function).
2. Update seed directory and quota targets.
3. Add benchmark cases to test the category.
4. Regenerate and qualify.
5. Update this document.

### Updating Benchmark Cases
1. Edit `build_benchmark()` in `build_base_lobe_v1_assets.py`.
2. Regenerate with `python3 scripts/build_base_lobe_v1_assets.py`.
3. Requalify with `./scripts/qualify_base_lobe_v1.sh`.
4. Commit seed and benchmark changes together.

---

## Performance Notes

**Current Baseline (v1 alpha, 713 facts):**
- Seeding: ~30 seconds (5 categories, 272 facts taught).
- Benchmarking: ~90 seconds (600 checks, ~150 ms/check average).
- Full qualification: ~2 minutes (build + server startup + seed + bench).
- Latency per query: < 10 ms (p50), < 25 ms (p95).

**Scaling Projections:**
- At 3K facts: seeding ~60s, benchmarking ~120s, qualification ~4 min.
- At 18K facts: seeding ~300s, benchmarking ~150s, qualification ~8 min.
- Memory: ~1 MB per 700 facts (linear).

---

## References

- [BASE_LOBE_V1_DESIGN.md](BASE_LOBE_V1_DESIGN.md) — Architectural decisions and philosophy.
- [BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md) — Formal specification (quotas, gates, thresholds).
- [BASE_LOBE_V1_PROCESS.md](BASE_LOBE_V1_PROCESS.md) — Step-by-step process documentation.
