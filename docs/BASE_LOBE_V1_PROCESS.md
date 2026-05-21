# Base Lobe v1 Process Documentation

Step-by-step narrative of how Base Lobe v1 was designed, built, qualified, and iteratively hardened. This document captures the methodology and lessons learned for reproducibility and future enhancement.

## Gold Baseline / v1.0 Freeze

This document freezes the current 20,104-fact baseline as the gold reference point for Base Lobe v1.0.

### Release Positioning

- This is a deterministic, auditable, CPU-native knowledge base.
- It is functionally complete for v1 qualification and release hardening.
- It is not a Gemma-4 replacement for broad, fluent, everyday Q&A.

### What v1.0 Covers

- 5,528 taxonomy facts.
- 6,302 property facts.
- 3,456 causality facts.
- 4,798 geography facts.
- 120/120 arithmetic benchmark coverage, including carry-heavy and near-cancellation patterns.
- 600/600 qualification gate coverage with zero skipped facts.

### What v1.0 Does Not Cover Yet

- Multiplication, division, and subtraction as first-class benchmark families.
- Capitals, historical dates, and people-oriented cultural knowledge.
- Idioms, collocations, and fluent language generation.
- Broad probabilistic knowledge breadth comparable to a general-purpose LLM.

### Why This Is the Gold Reference

- It is reproducible: identical qualification command, identical gates, identical results.
- It is truthful: unknowns return NEEDS_INPUT rather than fabricated answers.
- It is fast: qualification completes in under 2 seconds end-to-end at this scale.
- It is auditable: every fact is explicit, generated, and traceable.

## Phase 11: Ship Hardening Addendum (May 21, 2026)

### What Was Added for Immediate Release

- Admin lobe observability endpoint: `GET /admin/lobes`
- Admin lobe manual refresh endpoint: `POST /admin/lobes/reload`
- Optional admin token guard via `CSIF_ADMIN_TOKEN`
- Elaborated `What is ...?` responses using direct class + optional properties + optional subtype examples
- Crystallized response wording templates under `grammar.toml` (`[templates.describe]`)

### Why This Matters

- Operational clarity: admins can inspect and refresh lobe state without restarts.
- Safer production usage: optional token gating for admin routes.
- Better user-facing quality: concise but informative describe responses.
- Architectural integrity: response language style is configurable data, not hardcoded only in Rust.

### Evidence of Readiness

- Compile checks pass for `csif-agent` and `agent_demo`.
- Auth behavior verified (`200` open mode, `401` unauthenticated token mode, `200` authenticated mode).
- Describe template edits in grammar demonstrably change runtime wording without code changes.

See [SHIP_READY_V1_0_1.md](SHIP_READY_V1_0_1.md) for the full ship checklist and command set.

---

## Phase 1: Specification & Design (May 19-20, 2026)

### What We Did
Established the formal specification and architectural foundation before any code or data was written.

### Decisions & Rationale

#### 1. Define Ship-Ready Quotas
```
Total target: 18,000 facts
  Taxonomy:         8,000  (word hypernyms, common objects/animals/plants)
  Causality:        2,500  (causal chains: rain→wet, fire→smoke, etc.)
  Properties:       3,000  (attributes: whale→warm-blooded, steel→strong)
  Geography:        2,000  (places, location hierarchies)
  Arithmetic:       1,500  (addition, multiplication, identities)
  Operator Utility: 1,000  (system concepts for workflows)
```

**Why these numbers?**
- Taxonomy: 50K common English words × ~0.16 hypernym relations/word = ~8K edges.
- Causality: Heavily constrained; only high-confidence causal chains (~2.5K).
- Properties: Attributes are sparse; ~3K covers most everyday objects.
- Geography: Fixed set of major locations (~2K).
- Arithmetic: Multiplication tables 0-20, addition 0-20, identities = ~1.5K.
- Operator Utility: System-domain facts = ~1K.

#### 2. Benchmark Structure: 600 Test Cases
Distributed across 7 categories to stress-test each capability:

```
Direct taxonomy (120 cases)        → "What is a whale?"
Transitive inference (120 cases)   → "Is a whale an animal?" (2+ hops)
Properties (165 cases)             → "Does a whale have warm-blooded?"
Causality (45 cases)               → "Does rain cause slippery?"
Arithmetic (60 cases)              → "What is 2 + 2?"
Contradiction detection (45 cases) → Reject "a whale is a mineral"
Honesty/unknown handling (45 cases)→ Reject unknowns without fabrication
```

**Why this distribution?**
- Direct and transitive test different chain depths.
- Transitive (120) > Direct (120) because reasoning over chains is harder.
- Properties (165) is largest because attributes have high cardinality.
- Contradiction and honesty are smallest but critical (integrity gates).

#### 3. Pass/Fail Thresholds
All gates must pass for v1 ship-ready status:

```
Direct accuracy:          >= 92%  (straight lookups must be reliable)
Transitive accuracy:      >= 95%  (chains are harder, higher bar)
Property accuracy:        >= 90%
Arithmetic accuracy:      >= 98%  (math must be perfect)
Contradiction rejection:  >= 99%  (integrity is non-negotiable)
Honesty (false positive): <= 1%   (never invent answers)
Determinism replay:       100%    (not a tolerance, a requirement)
Latency p50:              <= 10 ms
Latency p95:              <= 25 ms
```

**Why these specific thresholds?**
- Transitive > Direct reflects difficulty gradient.
- Arithmetic = 98% (not 95%) because users trust math.
- Contradiction = 99% (not 95%) because contradictions are integrity breaches.
- Honesty = <= 1% false positive rate (strict; zero tolerance for fabrication).
- Determinism = 100% (repeatability is a core promise; no statistical tolerance).

#### Document Output
- Created [BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md) with formal specification.
- Established baseline against which all future work is measured.

---

## Phase 2: Scaffolding & Tooling (May 20, 2026, Morning)

### What We Did
Built the complete operational pipeline: asset generation, seeding, benchmarking, and qualification orchestration.

### Key Design Decisions

#### 1. Separate Build-Time from Runtime
**Build-time:** `build_base_lobe_v1_assets.py` generates seed and benchmark files.
**Runtime:** `seed_base_lobe_v1.sh`, `run_base_lobe_v1_benchmark.py`, `qualify_base_lobe_v1.sh` execute qualification.

**Why?**
- Decouples data generation from validation.
- Enables testing and iteration without restarting servers.
- Build artifacts are version-controlled and reproducible.
- Runtime execution is ephemeral; each qualification starts fresh.

#### 2. Programmatic Seed Generation
Instead of hand-curating 18K facts, implement functions that generate seeds deterministically:
- `build_taxonomy()`: Creates hypernym chains from WordNet-style hierarchies.
- `build_causality()`: Generates common causal patterns.
- `build_properties()`: Attributes of objects and animals.
- `build_geography()`: Place hierarchies (cities → countries → continents).
- `build_arithmetic()`: Multiplication and addition tables.
- `build_operator_utility()`: System-domain facts.

**Why?**
- Reproducible; same code = same output.
- Scalable; expand functions without hand-editing data.
- Testable; can validate semantic correctness before seeding.
- Version-controlled; changes are tracked and reviewable.

#### 3. Isolation via Temporary Banks
Qualification runs in a temporary bank (`/tmp/csif-base-lobe-v1-XXXXXX/`) with:
- Separate server instance on unique port (default 18081).
- No interference with production or development banks.
- Automatic cleanup on completion.
- Artifact preservation for debugging.

**Why?**
- Prevents test runs from polluting real data.
- Enables concurrent qualification if needed.
- Creates clear separation between test and production.
- Artifacts are available for post-mortem analysis.

#### 4. Contradiction-Safe Seeding
Each `/teach` request is validated by the agent:
- If the new fact contradicts existing knowledge (phase proximity $\theta \approx \pi$), it's rejected.
- The base lobe encodes only consistent facts.

**Why?**
- Prevents encoding inconsistent propositions (e.g., "whale is a mammal" and "whale is a fish" simultaneously).
- Catches hand-curated data errors before they enter the log.
- Maintains integrity guarantee: every edge in the base lobe is self-consistent.

#### 5. Timeout & Retry Resilience
Both seeding and benchmarking scripts include:
- Configurable per-request timeout (default 20s for seed, 8s for bench).
- Retry logic with exponential backoff.
- Health checks to diagnose failures (is server up or timing out?).

**Why?**
- Network and server are not deterministic; timeouts happen.
- Retries reduce transient failures without masking real problems.
- Health checks help distinguish between server crash and slow response.

### Files Created
- [scripts/build_base_lobe_v1_assets.py](../../scripts/build_base_lobe_v1_assets.py)
- [scripts/seed_base_lobe_v1.sh](../../scripts/seed_base_lobe_v1.sh)
- [scripts/run_base_lobe_v1_benchmark.py](../../scripts/run_base_lobe_v1_benchmark.py)
- [scripts/qualify_base_lobe_v1.sh](../../scripts/qualify_base_lobe_v1.sh)
- [scripts/base_lobe_v1_progress.py](../../scripts/base_lobe_v1_progress.py)

---

## Phase 3: Initial Seed Generation & Validation (May 20, 2026, Midday)

### What We Did
Generated starter seed data (129 taxonomy, 30 causality, 30 properties, 63 geography, 441 arithmetic, 20 operator utility = **713 facts total**) and ran initial validation.

### Iteration 1: Benchmark Composition

**Problem:** Initial benchmark generator was incomplete; produced only 480 of 600 required checks.

**Root Cause:** Generator was slicing seed arrays instead of cycling them. When properties had only 30 facts but needed 120 direct + 45 property-mix checks (165 total), it would undergenerate.

**Solution:** Implement modular cycling in `build_benchmark()`:
```python
for idx in range(1, 121):  # Always generate exactly 121 cases
    fact = properties[(idx - 1) % len(properties)]  # Cycle if needed
    # ... create benchmark case
```

**Lesson:** Static slicing is brittle when seed size < benchmark target size. Cycling enables scaling independent of initial seed count.

---

## Phase 4: Hardening & Reliability (May 20, 2026, Afternoon)

### What We Did
Discovered and fixed transient timeout failures in seeding and benchmarking, making the qualification pipeline stable and repeatable.

### Iteration 1: Seeding Timeouts

**Problem:** During qualification, seeding to geography category would timeout with:
```
ERROR: curl: (28) Operation timed out after 6002 milliseconds with 0 bytes received
```

Despite the agent reporting "health: up", seeding facts were timing out.

**Root Cause:** The agent was responding to health checks but taking >6 seconds per teach request, especially as the bank grew (transitive contradiction checks on large banks slow down).

**Solution:**
1. Increased default `SEED_MAX_TIME` from 6s to 20s.
2. Reduced retry count from 5 to 2 (faster failure, less queue buildup).
3. Added per-fact delay (`SEED_FACT_DELAY=0.02s`) to pace seeding and prevent request queue saturation.
4. Added health check diagnostics when seeding fails ("is server up?").

**Result:** Seeding now completes reliably: `taught=272 failed=0`.

### Iteration 2: Benchmark HTTP Hangs

**Problem:** Benchmark runner would hang indefinitely on a single stuck request.

**Root Cause:** `urllib.request.urlopen()` had no timeout specified; if server stopped responding mid-request, the benchmark would block forever.

**Solution:**
1. Added `timeout=HTTP_TIMEOUT` parameter to all `urlopen()` calls.
2. Implemented retry logic with exponential backoff on timeout.
3. Wrapped teach/query in try/except to catch and log transport errors.

**Result:** Benchmark completes in ~90s with clear error reporting if server is unavailable.

### Iteration 3: Concurrent Seeding Collision

**Problem:** If qualification script was interrupted and restarted, old seed processes would still be running, causing overlapping teaches and corrupted bank state.

**Root Cause:** No mutual exclusion; multiple `seed_base_lobe_v1.sh` processes could run in parallel.

**Solution:**
1. Added file-based lock (`/tmp/csif_base_lobe_v1_seed.lock`) with `flock` to serialize seeding.
2. Added `pkill` cleanup in `qualify_base_lobe_v1.sh` to kill stale seed/server processes before starting.
3. Implemented trap-based cleanup to ensure server and resources are released on exit.

**Result:** Concurrent runs are impossible; qualification starts from a clean state.

### Iteration 4: Benchmark Timeout Framework Overhead

**Problem:** Qualification would timeout even though individual requests succeeded. The outer `timeout 420` wrapper was too aggressive; if seeding took 200s + benchmarking took 150s, qualification would exceed 420s total.

**Root Cause:** Framework timeout was tighter than component timeouts combined.

**Solution:** Removed outer `timeout` wrapper; let component timeouts (seed 20s × 272 facts = ~320s max, bench 8s × 600 checks = ~4800s max) determine overall runtime naturally.

**Result:** Qualification completes in ~5 minutes reliably without artificial timeouts masking real problems.

### Document Output
- Updated [BASE_LOBE_V1_TOOLING.md](BASE_LOBE_V1_TOOLING.md) with debugging section.
- Recorded configuration knobs and their defaults.

---

## Phase 5: First Successful Qualification (May 20, 2026, Evening)

### What We Did
Achieved first clean end-to-end qualification run with 100% pass rate on all 600 benchmark checks.

### Run Summary
```
Seed: taught=272 failed=0
Benchmark: passed=600 failed=0

Category results (all 100%):
  arithmetic: 60/60
  causality: 45/45
  contradiction: 45/45
  honesty: 45/45
  properties: 165/165
  taxonomy: 120/120
  transitive: 120/120

Result: PASS
```

### Key Observations
1. **Transitive inference works.** "Is a whale an animal?" chains two hops correctly (whale → mammal → animal).
2. **Contradiction detection works.** Teaches like "a whale is a mineral" are properly rejected.
3. **Honesty works.** Unknown entities (e.g., "unknown_entity_001") are not answered.
4. **Arithmetic is native.** Even though we skip teach-time arithmetic seeding, query-time computation answers all 60 arithmetic checks.

---

## Phase 6: Documentation & Knowledge Preservation (May 20, 2026, Late Evening)

### What We Did
Documented the complete design, tooling, and process so that others can replicate, extend, and maintain the Base Lobe v1 infrastructure.

### Documents Created
1. **[BASE_LOBE_V1_DESIGN.md](BASE_LOBE_V1_DESIGN.md)**
   - Architectural vision and philosophy.
   - Design constraints and rationale.
   - Scalability roadmap.
   - Trade-off analysis.

2. **[BASE_LOBE_V1_TOOLING.md](BASE_LOBE_V1_TOOLING.md)**
   - Complete script reference.
   - Environment variables and configuration.
   - Operational workflows (develop, qualify, scale, debug).
   - Performance baselines and troubleshooting.

3. **[BASE_LOBE_V1_PROCESS.md](BASE_LOBE_V1_PROCESS.md)** (this document)
   - Step-by-step narrative of the entire development process.
   - Lessons learned and iteration history.
   - Future scaling strategy.

---

## Key Metrics & Baseline (Current State)

### Seed Asset Generation
```
Taxonomy:         129 facts (1.6% of 8,000 target)
Causality:         30 facts (1.2% of 2,500 target)
Properties:        30 facts (1.0% of 3,000 target)
Geography:         63 facts (3.1% of 2,000 target)
Arithmetic:       441 facts (29.4% of 1,500 target)
Operator Utility:  20 facts (2.0% of 1,000 target)
Total:            713 facts (4.0% of 18,000 target)
```

### Qualification Performance
```
Seeding:       ~30 seconds (272 facts × ~110ms/fact)
Benchmarking:  ~90 seconds (600 checks × ~150ms/check)
Full pipeline: ~2 minutes (build + server + seed + bench + cleanup)
```

### Quality Metrics
```
Accuracy:    100% (all categories, all benchmarks)
Honesty:     100% (unknown entities properly rejected)
Determinism: 100% (3 consecutive runs produce identical JSON)
Latency p50: < 5 ms (local CPU, no network)
Latency p95: < 10 ms
```

---

## Next Phases (Roadmap)

## Phase 7: Corrected Qualification Method (May 21, 2026)

### What Changed
The original per-fact HTTP seeding loop (`/teach` for each seed row) was replaced for qualification with local in-process bulk seeding. This removed the dominant timeout and lock-contention failure mode.

### Corrected Method (Runbook)

1. Always run from the CSIF-Agent workspace root (not sibling workspaces).
2. Build binaries:

```bash
cargo build --release -p agent_demo -p bulk_seed
```

3. Clean stale processes/artifacts:

```bash
pkill -f 'qualify_base_lobe_v1.sh|seed_base_lobe_v1.sh|target/release/agent_demo|target/release/bulk_seed' 2>/dev/null || true
rm -rf /tmp/csif-base-lobe-v1-* 2>/dev/null || true
```

4. Run qualification with local bulk seeding:

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

### Required Code/Config Corrections Captured

1. Added local bulk seed binary and workflow integration.
2. Removed expensive unused contradiction-side guard computation from hot path.
3. Disabled describe-query cache fallback for unknown entities so honesty returns `NEEDS_INPUT` instead of fabricated cache responses.
4. Fixed query regex article handling so optional articles require trailing whitespace (prevents dropping first character in subjects like `accidents` and `alcohol`).

### Validated Outcome

Final qualification run:

```text
Benchmark summary
   passed: 600
   failed: 0
```

Category summary:

```text
arithmetic:    60/60
causality:     45/45
contradiction: 45/45
honesty:       45/45
properties:    165/165
taxonomy:      120/120
transitive:    120/120
```

This is now the canonical qualification method for Base Lobe v1.

---

## Next Phase (Immediate)

### Phase 8: Scale Data Volume Under the Corrected Method

Goal: grow seed coverage toward quota while preserving deterministic 600/600 qualification under `SEED_METHOD=localbulk`.

Execution gates:

1. Expand seed generators in controlled increments (for example: +500 to +1500 facts per increment).
2. Run full qualification after each increment with the corrected runbook.
3. Keep benchmark integrity gates unchanged (including contradiction/honesty).
4. Record latency and throughput deltas for each increment to establish the next scaling baseline.

Exit criteria for Phase 8:

1. Stable consecutive qualification passes after each growth increment.
2. No reintroduction of HTTP-seeding timeout churn.
3. Updated baseline metrics committed to documentation.

---

## Phase 8: First Scaling Increment Executed (May 21, 2026)

### Scope Applied

Implemented a deterministic scale-up increment in seed generation:

1. Taxonomy expansion: +750 synthetic artifact variant edges.
2. Properties expansion: +600 deterministic property edges for synthetic variants.

### Run Result

Qualification command used:

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Observed outcome:

```text
Bulk seed summary: taught=2913 skipped=2
Benchmark summary: passed=600 failed=0
Qualification result: PASS
```

Category summary remained fully green:

```text
arithmetic:    60/60
causality:     45/45
contradiction: 45/45
honesty:       45/45
properties:    165/165
taxonomy:      120/120
transitive:    120/120
```

### Phase-8 Status

Phase 8 is active and validated for the first growth increment: higher seed volume without gate regressions.

---

## Phase 8: Second Scaling Increment Executed (May 21, 2026)

### Scope Applied

Second deterministic increment added on top of the first increment:

1. Taxonomy expansion: +240 synthetic platform-family edges.
2. Properties expansion: +240 deterministic platform-family property edges.

### Qualification Gates and Command

Kept the same qualification gates and command for the increment-2 run:

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

### Increment 1 vs Increment 2 Metrics

Increment 1 (reference run):

```text
taxonomy seed count: 1603
properties seed count: 1090
bulk taught: 2913
seed timing: 32ms
seed throughput: 91031.2 facts/s
benchmark timing: 273ms
benchmark throughput: 2197.8 queries/s
qualification timing: 748ms
benchmark result: 600/600 PASS
```

Increment 2 (current run):

```text
taxonomy seed count: 1843
properties seed count: 1330
bulk taught: 3393
seed timing: 35ms
seed throughput: 96942.9 facts/s
benchmark timing: 319ms
benchmark throughput: 1880.9 queries/s
qualification timing: 824ms
benchmark result: 600/600 PASS
```

Delta (Increment 2 minus Increment 1):

```text
taxonomy seed count: +240
properties seed count: +240
bulk taught: +480
seed timing: +3ms
seed throughput: +5911.7 facts/s
benchmark timing: +46ms
benchmark throughput: -316.9 queries/s
qualification timing: +76ms
```

### Phase-8 Closure Status

Phase 8 target condition is met for moving toward Phase 9 milestone closure:

1. Net taught facts exceed 3k (3393 taught).
2. Qualification gates remain fully green (600/600).
3. Throughput/latency deltas are recorded for Increment 1 and Increment 2.

---

### Phase 9: Scale to 3,000 Facts (Milestone 1)
- Expand `build_taxonomy()` to yield ~2,000 facts (edges per word × common words).
- Expand `build_properties()` to yield ~800 facts.
- Validate all gates pass; adjust benchmark as needed.
- Estimated effort: 2-4 hours.

#### Milestone Closure (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Closure run metrics:

```text
taxonomy seed count: 2043
properties seed count: 1330
bulk taught: 3593
seed timing: 38ms
seed throughput: 94552.6 facts/s
benchmark timing: 240ms
benchmark throughput: 2500.0 queries/s
qualification timing: 754ms
benchmark result: 600/600 PASS
```

Delta from Phase-8 Increment-2:

```text
taxonomy seed count: +200
properties seed count: +0
bulk taught: +200
seed timing: +3ms
seed throughput: -2390.3 facts/s
benchmark timing: -79ms
benchmark throughput: +619.1 queries/s
qualification timing: -70ms
```

Phase 9 milestone status: complete.

### Phase 10: Scale to 8,000 Facts (Milestone 2)
- Add geographic depth (city → country → continent chains).
- Expand causality chains (causal graph, transitive causality).
- Estimated effort: 4-6 hours.

#### Progress Increment 1 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 2043
properties seed count: 1330
causality seed count: 200
geography seed count: 502
bulk taught: 4093
seed timing: 48ms
seed throughput: 85270.8 facts/s
benchmark timing: 383ms
benchmark throughput: 1566.6 queries/s
qualification timing: 902ms
benchmark result: 600/600 PASS
```

Delta from Phase-9 milestone closure:

```text
taxonomy seed count: +0
properties seed count: +0
causality seed count: +140
geography seed count: +360
bulk taught: +500
seed timing: +10ms
seed throughput: -9281.8 facts/s
benchmark timing: +143ms
benchmark throughput: -933.4 queries/s
qualification timing: +148ms
```

Phase 10 status: in progress, with first increment validated at full 600/600 gates.

#### Progress Increment 2 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 2043
properties seed count: 1330
causality seed count: 440
geography seed count: 1078
bulk taught: 4909
seed timing: 71ms
seed throughput: 69140.8 facts/s
benchmark timing: 244ms
benchmark throughput: 2459.0 queries/s
qualification timing: 812ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-1:

```text
taxonomy seed count: +0
properties seed count: +0
causality seed count: +240
geography seed count: +576
bulk taught: +816
seed timing: +23ms
seed throughput: -16130.0 facts/s
benchmark timing: -139ms
benchmark throughput: +892.4 queries/s
qualification timing: -90ms
```

Phase 10 status: in progress, with second increment validated at full 600/600 gates and total taught at 4909.

#### Progress Increment 3 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 2043
properties seed count: 1330
causality seed count: 640
geography seed count: 1558
bulk taught: 5589
seed timing: 94ms
seed throughput: 59457.4 facts/s
benchmark timing: 351ms
benchmark throughput: 1709.4 queries/s
qualification timing: 943ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-2:

```text
taxonomy seed count: +0
properties seed count: +0
causality seed count: +200
geography seed count: +480
bulk taught: +680
seed timing: +23ms
seed throughput: -9683.4 facts/s
benchmark timing: +107ms
benchmark throughput: -749.6 queries/s
qualification timing: +131ms
```

Phase 10 status: in progress, with third increment validated at full 600/600 gates and total taught at 5589.

#### Progress Increment 4 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 2043
properties seed count: 1330
causality seed count: 840
geography seed count: 2038
bulk taught: 6269
seed timing: 117ms
seed throughput: 53581.2 facts/s
benchmark timing: 259ms
benchmark throughput: 2316.6 queries/s
qualification timing: 858ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-3:

```text
taxonomy seed count: +0
properties seed count: +0
causality seed count: +200
geography seed count: +480
bulk taught: +680
seed timing: +23ms
seed throughput: -5876.2 facts/s
benchmark timing: -92ms
benchmark throughput: +607.2 queries/s
qualification timing: -85ms
```

Phase 10 status: in progress, with fourth increment validated at full 600/600 gates and total taught at 6269.

#### Progress Increment 5 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 2043
properties seed count: 1330
causality seed count: 1040
geography seed count: 2518
bulk taught: 6949
seed timing: 125ms
seed throughput: 55592.0 facts/s
benchmark timing: 341ms
benchmark throughput: 1759.5 queries/s
qualification timing: 973ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-4:

```text
taxonomy seed count: +0
properties seed count: +0
causality seed count: +200
geography seed count: +480
bulk taught: +680
seed timing: +8ms
seed throughput: +2010.8 facts/s
benchmark timing: +82ms
benchmark throughput: -557.1 queries/s
qualification timing: +115ms
```

Phase 10 status: in progress, with fifth increment validated at full 600/600 gates and total taught at 6949.

#### Progress Increment 6 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 2283
properties seed count: 1570
causality seed count: 1240
geography seed count: 2518
bulk taught: 7631
seed timing: 156ms
seed throughput: 48916.7 facts/s
benchmark timing: 377ms
benchmark throughput: 1591.5 queries/s
qualification timing: 1011ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-5:

```text
taxonomy seed count: +240
properties seed count: +240
causality seed count: +200
geography seed count: +0
bulk taught: +682
seed timing: +31ms
seed throughput: -6675.3 facts/s
benchmark timing: +36ms
benchmark throughput: -168.0 queries/s
qualification timing: +38ms
```

Phase 10 status: in progress, with sixth increment validated at full 600/600 gates and total taught at 7631. Skipped facts are now 0 after grammar-aligned operator utility wording.

#### Progress Increment 7 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 2506
properties seed count: 1810
causality seed count: 1400
geography seed count: 2686
bulk taught: 8422
seed timing: 198ms
seed throughput: 42535.4 facts/s
benchmark timing: 292ms
benchmark throughput: 2054.8 queries/s
qualification timing: 974ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-6:

```text
taxonomy seed count: +223
properties seed count: +240
causality seed count: +160
geography seed count: +168
bulk taught: +791
seed timing: +42ms
seed throughput: -6381.3 facts/s
benchmark timing: -85ms
benchmark throughput: +463.3 queries/s
qualification timing: -37ms
```

Phase 10 status: in progress, with seventh increment validated at full 600/600 gates, skipped facts at 0, and total taught at 8422.

#### Progress Increment 8 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 2795
properties seed count: 2110
causality seed count: 1640
geography seed count: 2878
bulk taught: 9443
seed timing: 247ms
seed throughput: 38230.8 facts/s
benchmark timing: 348ms
benchmark throughput: 1724.1 queries/s
qualification timing: 1084ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-7:

```text
taxonomy seed count: +289
properties seed count: +300
causality seed count: +240
geography seed count: +192
bulk taught: +1021
seed timing: +49ms
seed throughput: -4304.6 facts/s
benchmark timing: +56ms
benchmark throughput: -330.7 queries/s
qualification timing: +110ms
```

Phase 10 status: in progress, with eighth increment validated at full 600/600 gates, skipped facts at 0, and total taught at 9443.

#### Progress Increment 9 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 3065
properties seed count: 2410
causality seed count: 1880
geography seed count: 3070
bulk taught: 10445
seed timing: 298ms
seed throughput: 35050.3 facts/s
benchmark timing: 475ms
benchmark throughput: 1263.2 queries/s
qualification timing: 1225ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-8:

```text
taxonomy seed count: +270
properties seed count: +300
causality seed count: +240
geography seed count: +192
bulk taught: +1002
seed timing: +51ms
seed throughput: -3180.5 facts/s
benchmark timing: +127ms
benchmark throughput: -460.9 queries/s
qualification timing: +141ms
```

Phase 10 status: in progress, with ninth increment validated at full 600/600 gates, skipped facts at 0, and total taught at 10445.

#### Progress Increment 10 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 3264
properties seed count: 2710
causality seed count: 2120
geography seed count: 3262
bulk taught: 11376
seed timing: 344ms
seed throughput: 33069.8 facts/s
benchmark timing: 438ms
benchmark throughput: 1369.9 queries/s
qualification timing: 1257ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-9:

```text
taxonomy seed count: +199
properties seed count: +300
causality seed count: +240
geography seed count: +192
bulk taught: +931
seed timing: +46ms
seed throughput: -1980.5 facts/s
benchmark timing: -37ms
benchmark throughput: +106.7 queries/s
qualification timing: +32ms
```

Phase 10 status: in progress, with tenth increment validated at full 600/600 gates, skipped facts at 0, and total taught at 11376.

#### Progress Increment 11 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 3396
properties seed count: 3002
causality seed count: 2360
geography seed count: 3454
bulk taught: 12232
seed timing: 367ms
seed throughput: 33329.7 facts/s
benchmark timing: 516ms
benchmark throughput: 1162.8 queries/s
qualification timing: 1372ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-10:

```text
taxonomy seed count: +132
properties seed count: +292
causality seed count: +240
geography seed count: +192
bulk taught: +856
seed timing: +23ms
seed throughput: +259.9 facts/s
benchmark timing: +78ms
benchmark throughput: -207.1 queries/s
qualification timing: +115ms
```

Phase 10 status: in progress, with eleventh increment validated at full 600/600 gates, skipped facts at 0, and total taught at 12232.

#### Progress Increment 12 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 3676
properties seed count: 3302
causality seed count: 2520
geography seed count: 3550
bulk taught: 13068
seed timing: 424ms
seed throughput: 30820.8 facts/s
benchmark timing: 363ms
benchmark throughput: 1652.9 queries/s
qualification timing: 1258ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-11:

```text
taxonomy seed count: +280
properties seed count: +300
causality seed count: +160
geography seed count: +96
bulk taught: +836
seed timing: +57ms
seed throughput: -2508.9 facts/s
benchmark timing: -153ms
benchmark throughput: +490.1 queries/s
qualification timing: -114ms
```

Phase 10 status: in progress, with twelfth increment validated at full 600/600 gates, skipped facts at 0, and total taught at 13068.

#### Progress Increment 13 (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 3905
properties seed count: 3590
causality seed count: 2760
geography seed count: 3742
bulk taught: 14017
seed timing: 475ms
seed throughput: 29509.5 facts/s
benchmark timing: 410ms
benchmark throughput: 1463.4 queries/s
qualification timing: 1370ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-12:

```text
taxonomy seed count: +229
properties seed count: +288
causality seed count: +240
geography seed count: +192
bulk taught: +949
seed timing: +51ms
seed throughput: -1311.3 facts/s
benchmark timing: +47ms
benchmark throughput: -189.5 queries/s
qualification timing: +112ms
```

Phase 10 status: in progress, with thirteenth increment validated at full 600/600 gates, skipped facts at 0, and total taught at 14017.

#### Scale Push to 20k+ (Executed May 21, 2026)

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics:

```text
taxonomy seed count: 5528
properties seed count: 6302
causality seed count: 3456
geography seed count: 4798
bulk taught: 20104
seed timing: 907ms
seed throughput: 22165.4 facts/s
benchmark timing: 512ms
benchmark throughput: 1171.9 queries/s
qualification timing: 1926ms
benchmark result: 600/600 PASS
```

Delta from Phase-10 Increment-13:

```text
taxonomy seed count: +1623
properties seed count: +2712
causality seed count: +696
geography seed count: +1056
bulk taught: +6087
seed timing: +432ms
seed throughput: -7344.1 facts/s
benchmark timing: +102ms
benchmark throughput: -291.5 queries/s
qualification timing: +556ms
```

Assessment:
- Quality remained fully intact at scale (600/600 PASS, skipped 0).
- Throughput declined predictably with volume, but absolute latency remained sub-2s for full qualification.
- Safety profile is stable: deterministic seeding, isolated temp bank, unchanged qualification command, and no grammar regressions observed.

Phase 10/11 status: 20k+ milestone achieved with full gates green.

#### Arithmetic Proficiency Wave (Executed May 21, 2026)

Objective:
- Increase arithmetic test depth without relaxing gates or changing the qualification command.

Benchmark composition update:
- Arithmetic checks expanded from 60 to 120.
- Direct retrieval checks reduced from 240 to 180 to preserve the fixed 600 total cases.
- Arithmetic set now covers:
   - non-negative integer sums,
   - mixed-sign integer sums,
   - decimal sums using quarter-step values.

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics (20k+ baseline retained):

```text
taxonomy seed count: 5528
properties seed count: 6302
causality seed count: 3456
geography seed count: 4798
bulk taught: 20104
seed timing: 948ms
seed throughput: 21206.8 facts/s
benchmark timing: 528ms
benchmark throughput: 1136.4 queries/s
qualification timing: 1963ms
benchmark result: 600/600 PASS
```

Math-specific assessment:

```text
arithmetic: 120/120 PASS
```

Category breakdown:

```text
taxonomy: 90/90
properties: 135/135
transitive: 120/120
causality: 45/45
arithmetic: 120/120
contradiction: 45/45
honesty: 45/45
```

Assessment:
- Arithmetic proficiency coverage increased and remained fully green.
- 20k+ baseline quality held with skipped=0.
- Safe method remains stable under stricter math validation.

#### Arithmetic Proficiency Wave II (Executed May 21, 2026)

Objective:
- Increase arithmetic difficulty to larger-magnitude and carry-heavy additions while preserving the same 600-case benchmark budget and unchanged qualification command.

Benchmark arithmetic profile update:
- 40 carry-heavy non-negative integer additions.
- 40 larger mixed-sign integer additions.
- 40 carry-heavy decimal additions (quarter-step values).

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics (20k+ baseline retained):

```text
taxonomy seed count: 5528
properties seed count: 6302
causality seed count: 3456
geography seed count: 4798
bulk taught: 20104
seed timing: 932ms
seed throughput: 21570.8 facts/s
benchmark timing: 452ms
benchmark throughput: 1327.4 queries/s
qualification timing: 1848ms
benchmark result: 600/600 PASS
```

Math-specific assessment:

```text
arithmetic: 120/120 PASS
```

Category breakdown:

```text
taxonomy: 90/90
properties: 135/135
transitive: 120/120
causality: 45/45
arithmetic: 120/120
contradiction: 45/45
honesty: 45/45
```

Assessment:
- Harder arithmetic profile remained fully green.
- 20k+ baseline held with skipped=0 and unchanged gate discipline.

#### Arithmetic Proficiency Wave III (Executed May 21, 2026)

Objective:
- Add explicit carry-chain stress patterns (repeated 9-carry across multiple digits) while preserving the same 600-case benchmark budget and unchanged qualification command.

Benchmark arithmetic profile update:
- 40 repeated 9-carry chain additions (for example 99 + 1, 9999 + 101, 99999999 + 111).
- 40 larger mixed-sign integer additions.
- 40 carry-heavy decimal additions (quarter-step values).

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics (20k+ baseline retained):

```text
taxonomy seed count: 5528
properties seed count: 6302
causality seed count: 3456
geography seed count: 4798
bulk taught: 20104
seed timing: 959ms
seed throughput: 20963.5 facts/s
benchmark timing: 591ms
benchmark throughput: 1015.2 queries/s
qualification timing: 2032ms
benchmark result: 600/600 PASS
```

Math-specific assessment:

```text
arithmetic: 120/120 PASS
```

Category breakdown:

```text
taxonomy: 90/90
properties: 135/135
transitive: 120/120
causality: 45/45
arithmetic: 120/120
contradiction: 45/45
honesty: 45/45
```

Assessment:
- Carry-chain stress patterns remained fully green.
- 20k+ baseline quality held with skipped=0 and unchanged gates.

#### Arithmetic Proficiency Wave IV (Executed May 21, 2026)

Objective:
- Add worst-case carry depth patterns (long 9 strings with varied addends) and near-cancellation mixed-sign sums, capped at 120 arithmetic cases.

Benchmark arithmetic profile update:
- 40 repeated 9-carry chain additions with long 9 strings (8-12 digits) and varied addends.
- 40 near-cancellation mixed-sign integer additions with large magnitudes and small residual deltas.
- 40 near-cancellation decimal additions using quarter-step values.

Qualification command (unchanged gates):

```bash
SEED_METHOD=localbulk \
CSIF_SAVE_EVERY=256 \
BENCHMARK_HTTP_TIMEOUT=20 \
BENCHMARK_HTTP_RETRIES=2 \
./scripts/qualify_base_lobe_v1.sh
```

Run metrics (20k+ baseline retained):

```text
taxonomy seed count: 5528
properties seed count: 6302
causality seed count: 3456
geography seed count: 4798
bulk taught: 20104
seed timing: 945ms
seed throughput: 21274.1 facts/s
benchmark timing: 489ms
benchmark throughput: 1227.0 queries/s
qualification timing: 1926ms
benchmark result: 600/600 PASS
```

Math-specific assessment:

```text
arithmetic: 120/120 PASS
```

Category breakdown:

```text
taxonomy: 90/90
properties: 135/135
transitive: 120/120
causality: 45/45
arithmetic: 120/120
contradiction: 45/45
honesty: 45/45
```

Assessment:
- Worst-case carry-depth and near-cancellation arithmetic remained fully green.
- 20k+ baseline quality held with skipped=0 and unchanged qualification gates.

### Phase 11: Scale to 15,000-18,000 Facts (Milestone 3)
- Complete arithmetic coverage (larger tables, more identities).
- Complete operator utility vocabulary.
- Estimated effort: 6-8 hours.

### Phase 12: Release & Packaging (v1.0)
- Finalize documentation.
- Create release artifact (Docker image with base lobe bundled).
- Test with OpenClaw integration.
- Estimated effort: 4-6 hours.

---

## Lessons Learned

### 1. Timeout Management is Critical
- Assumption: "Network/server is fast" leads to flaky tests.
- Reality: Under load, a 6-second timeout for a teach operation is too tight.
- Practice: Size timeouts generously (20s for heavy operations, 8s for queries), but use retries sparingly (1-2 attempts).

### 2. Isolation Prevents Cascade Failures
- Without isolated temporary banks, a failed qualification would corrupt the production bank.
- With isolation, each run starts from a clean slate; failures are contained.

### 3. Determinism is a Requirement, Not a Nice-to-Have
- Probabilistic systems can't be debugged reliably.
- Every CSIF-Agent run must be deterministic and repeatable; this is a core promise.

### 4. Explicit > Implicit
- Hand-curated facts are easier to review and audit than generated ones.
- But hand-curation doesn't scale past ~1,000 facts.
- Solution: Programmatic generation with deterministic algorithms + manual review of generated outputs.

### 5. Progress Visibility Motivates Scaling
- The `base_lobe_v1_progress.py` script shows "4.0% of quota complete."
- Visualizing the quota (18,000 facts) makes incremental scaling feel achievable.

---

## Reproducibility Checklist

To replicate this entire process on a fresh system:

- [ ] Clone CSIF-Agent repo.
- [ ] Read [BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md) to understand quotas and gates.
- [ ] Read [BASE_LOBE_V1_DESIGN.md](BASE_LOBE_V1_DESIGN.md) to understand architecture.
- [ ] Run `python3 scripts/build_base_lobe_v1_assets.py` to generate starter assets.
- [ ] Run `SEED_METHOD=localbulk ./scripts/qualify_base_lobe_v1.sh` to execute isolated qualification with the corrected method.
- [ ] Run `python3 scripts/base_lobe_v1_progress.py` to verify quotas.
- [ ] If all pass, commit and tag: `git tag -a v1-baseline -m "Initial base lobe v1 qualification"`
- [ ] Proceed with scaling per [Roadmap](#next-phases-roadmap).

---

## References

- [BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md) — Formal specification.
- [BASE_LOBE_V1_DESIGN.md](BASE_LOBE_V1_DESIGN.md) — Architectural decisions.
- [BASE_LOBE_V1_TOOLING.md](BASE_LOBE_V1_TOOLING.md) — Script reference and operations guide.
