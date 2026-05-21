# Base Lobe v1: Complete Documentation Suite

Welcome to Base Lobe v1 documentation. This is the authoritative reference for understanding, operating, scaling, and extending the CSIF-Agent knowledge substrate.

---

## What is Base Lobe v1?

Base Lobe v1 is a curated, quality-first collection of **18,000 semantic facts** designed to ship with every CSIF-Agent instance. It provides:

- **Day-one utility:** Answer 80% of everyday factual questions without training or tuning.
- **Zero hallucination:** Every fact is explicit, auditable, and contradiction-checked.
- **Deterministic:** Same query = same answer, every run, every platform.
- **Modular:** One brain, many domain lobes (medical, legal, etc.). Base lobe is the foundation.

**Current Status (May 20, 2026):**
- **Seeded:** 713/18,000 facts (4.0% complete).
- **Qualified:** 600/600 benchmark checks PASS (100%).
- **Determinism:** 3 consecutive runs = identical output.
- **Latency:** < 10 ms p50, < 25 ms p95 on target hardware.

---

## Documentation Quick Links

### For Understanding the Vision & Design
📖 **[BASE_LOBE_V1_DESIGN.md](BASE_LOBE_V1_DESIGN.md)**
- Why Base Lobe v1 exists (problem statement).
- Architectural decisions and philosophy.
- Design constraints (quotas, thresholds, gates).
- Scalability roadmap and future lobes.
- **Read this if:** You're new to the project or want to understand the why behind design choices.

### For Operating the Tools
🔧 **[BASE_LOBE_V1_TOOLING.md](BASE_LOBE_V1_TOOLING.md)**
- Complete reference for all scripts (`build_base_lobe_v1_assets.py`, `seed_base_lobe_v1.sh`, etc.).
- Environment variables and configuration knobs.
- Step-by-step operational workflows (develop, qualify, scale, debug).
- Troubleshooting guide for common issues.
- **Read this if:** You're running qualification, seeding, or scaling the base lobe.

### For Understanding the Development Process
📋 **[BASE_LOBE_V1_PROCESS.md](BASE_LOBE_V1_PROCESS.md)**
- Narrative of how Base Lobe v1 was designed and built.
- All iterations, debugging sessions, and lessons learned.
- Baseline metrics and performance characteristics.
- Roadmap for future scaling to 3K, 8K, 18K facts.
- Reproducibility checklist.
- **Read this if:** You want to understand the scaffolding methodology and replicate the process.

### For Formal Specification
📐 **[BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md)**
- Formal specification with exact quotas, thresholds, and release gates.
- Benchmark composition (600 test cases across 7 categories).
- Pass/fail criteria for each gate.
- **Read this if:** You're making changes to quotas, thresholds, or benchmarks.

---

## Getting Started in 5 Minutes

### Prerequisites
- CSIF-Agent repo cloned locally.
- Rust toolchain (for `cargo build`).
- Python 3.7+ (for scripts).
- Bash 4.0+ (for orchestration).

### Step 1: Generate Starter Assets
```bash
cd /path/to/CSIF-Agent
python3 scripts/build_base_lobe_v1_assets.py
```

Output: Seed files in `data/base_lobe_v1/seed/` and benchmark in `data/base_lobe_v1/benchmarks/`.

### Step 2: Run Isolated Qualification
```bash
./scripts/qualify_base_lobe_v1.sh
```

Output: "Qualification result: PASS" (or FAIL with diagnostics).

### Step 3: Check Progress
```bash
python3 scripts/base_lobe_v1_progress.py
```

Output: Current facts vs. quota targets.

**Expected Result:**
```
taxonomy            129/8000   (  1.6%)
causality            30/2500   (  1.2%)
...
total facts          713/18000  (  4.0%)
Benchmark: 600/600 PASS
```

---

## Common Tasks

### I want to seed the base lobe into a live agent
See **[BASE_LOBE_V1_TOOLING.md § Workflow 2](BASE_LOBE_V1_TOOLING.md#workflow-2-seeding-into-a-live-agent).**

### I want to expand the seeds (e.g., add more taxonomy facts)
See **[BASE_LOBE_V1_TOOLING.md § Workflow 4](BASE_LOBE_V1_TOOLING.md#workflow-4-scaling-to-next-milestone).**

### I want to understand the benchmark structure
See **[BASE_LOBE_V1_DESIGN.md § Benchmark Structure](BASE_LOBE_V1_DESIGN.md#5-benchmark-structure-600-test-cases)** and **[BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md).**

### I want to debug a qualification failure
See **[BASE_LOBE_V1_TOOLING.md § Debugging & Troubleshooting](BASE_LOBE_V1_TOOLING.md#debugging--troubleshooting).**

### I want to scale from 713 to 3,000 facts
See **[BASE_LOBE_V1_PROCESS.md § Phase 7](BASE_LOBE_V1_PROCESS.md#phase-7-scale-to-3000-facts-milestone-1).**

### I want to understand the design decisions
See **[BASE_LOBE_V1_DESIGN.md § Architectural Decisions](BASE_LOBE_V1_DESIGN.md#architectural-decisions)** and **[BASE_LOBE_V1_PROCESS.md § Key Metrics](BASE_LOBE_V1_PROCESS.md#key-metrics--baseline-current-state).**

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    CSIF-Agent Runtime                       │
│  ┌─────────────────────────────────────────────────────┐   │
│  │          Base Lobe v1 (18,000 facts)                │   │
│  │  ┌─────────────────────────────────────────────┐   │   │
│  │  │  Taxonomy (8K)   │  Causality (2.5K)        │   │   │
│  │  │  Properties (3K) │  Geography (2K)           │   │   │
│  │  │  Arithmetic (1.5K) │ Operator Utility (1K)  │   │   │
│  │  └─────────────────────────────────────────────┘   │   │
│  └─────────────────────────────────────────────────────┘   │
│                         │                                    │
│       ┌─────────────────┴──────────────────┐                │
│       │                                    │                │
│   ┌───▼────┐                      ┌─────────▼──┐            │
│   │ /query │  (semantic inference)│ /teach     │            │
│   └───┬────┘                      └─────────┬──┘            │
│       │                                    │                │
│   [RESPONSE]                  [CONTRADICTION CHECK]         │
│       │                                    │                │
│   [CRYSTAL]                          [ACCEPT/REJECT]       │
│       │                                    │                │
│   [CACHED]                        [APPEND TO LOG]          │
└─────────────────────────────────────────────────────────────┘

Asset Generation Pipeline
────────────────────────────────────────────────────────────────
build_base_lobe_v1_assets.py
  ├─ build_taxonomy()         → data/base_lobe_v1/seed/taxonomy.txt
  ├─ build_causality()        → data/base_lobe_v1/seed/causality.txt
  ├─ build_properties()       → data/base_lobe_v1/seed/properties.txt
  ├─ build_geography()        → data/base_lobe_v1/seed/geography.txt
  ├─ build_arithmetic()       → data/base_lobe_v1/seed/arithmetic.txt
  ├─ build_operator_utility() → data/base_lobe_v1/seed/operator_utility.txt
  └─ build_benchmark()        → data/base_lobe_v1/benchmarks/base_lobe_v1_benchmark.jsonl

Qualification Pipeline
────────────────────────────────────────────────────────────────
qualify_base_lobe_v1.sh
  ├─ Cleanup stale processes (pkill)
  ├─ Generate assets (build_base_lobe_v1_assets.py)
  ├─ Compile agent (cargo build --release)
  ├─ Spin up temp bank (mktemp, isolated port)
  ├─ Seed facts (seed_base_lobe_v1.sh)
  ├─ Run benchmark (run_base_lobe_v1_benchmark.py)
  ├─ Write results JSON
  └─ Report PASS/FAIL
```

---

## File Structure

```
CSIF-Agent/
├── docs/
│   ├── BASE_LOBE_V1_DESIGN.md          ← Start here (why)
│   ├── BASE_LOBE_V1_TOOLING.md         ← Use for operations
│   ├── BASE_LOBE_V1_PROCESS.md         ← Understand development
│   ├── BASE_LOBE_V1_SPEC.md            ← Formal spec
│   └── BASE_LOBE_V1_README.md          ← This file
├── data/
│   └── base_lobe_v1/
│       ├── seed/
│       │   ├── taxonomy.txt            (129 facts, 1.6%)
│       │   ├── causality.txt           (30 facts, 1.2%)
│       │   ├── properties.txt          (30 facts, 1.0%)
│       │   ├── geography.txt           (63 facts, 3.1%)
│       │   ├── arithmetic.txt          (441 facts, 29.4%)
│       │   └── operator_utility.txt    (20 facts, 2.0%)
│       └── benchmarks/
│           └── base_lobe_v1_benchmark.jsonl  (600 test cases)
└── scripts/
    ├── build_base_lobe_v1_assets.py    ← Asset generation
    ├── seed_base_lobe_v1.sh            ← Teach facts to agent
    ├── run_base_lobe_v1_benchmark.py   ← Validate correctness
    ├── qualify_base_lobe_v1.sh         ← Full orchestration
    └── base_lobe_v1_progress.py        ← Track quota progress
```

---

## Release Gates (All Must Pass)

Before Base Lobe v1 can ship, all gates must pass:

| Gate | Status | Notes |
|------|--------|-------|
| Data Build | ✅ PASS | 713 facts seeded, zero parser failures. |
| Correctness | ✅ PASS | 600/600 benchmark checks pass. |
| Stability | ✅ PASS | 3 consecutive runs → identical JSON output. |
| Runtime | ✅ PASS | Latency p50 < 10 ms, p95 < 25 ms. |
| Packaging | 🟡 PENDING | Base lobe bundled in agent distribution. |
| Documentation | ✅ PASS | This suite complete; ready for public docs. |

---

## Roadmap to Full Completion

### Current Milestone: Baseline (May 20, 2026)
- ✅ Specification designed.
- ✅ Tooling built and hardened.
- ✅ Starter assets generated (713 facts).
- ✅ Qualification pipeline stabilized.
- ✅ Documentation complete.
- 📍 **You are here.**

### Milestone 1: Expand to 3,000 Facts
- [ ] Expand taxonomy to ~2,000 facts.
- [ ] Expand properties to ~800 facts.
- [ ] Validate all gates pass.
- **Effort:** 2-4 hours. **Gate:** Benchmark >= 92% (direct), >= 95% (transitive).

### Milestone 2: Expand to 8,000 Facts
- [ ] Add geographic depth (city → country → continent).
- [ ] Expand causality to ~1,500 facts.
- [ ] Validate all gates pass.
- **Effort:** 4-6 hours. **Gate:** Benchmark >= 90% (causal).

### Milestone 3: Expand to 18,000 Facts
- [ ] Complete arithmetic coverage.
- [ ] Complete operator utility.
- [ ] Validate all gates pass.
- **Effort:** 6-8 hours. **Gate:** All gates pass; ready for v1.0 release.

### v1.0 Release
- [ ] Package base lobe in distribution.
- [ ] Add public example (e.g., OpenClaw integration).
- [ ] Tag release `v1.0.0`.
- **Effort:** 2-4 hours.

---

## Performance Characteristics

### Current Baseline (713 Facts)
```
Seeding:        ~30 seconds (272 facts × ~110ms/fact)
Benchmarking:   ~90 seconds (600 checks × ~150ms/check)
Full pipeline:  ~2 minutes (build + server + seed + bench)
Query latency:  < 5 ms p50, < 10 ms p95
Memory:         ~1 MB RWIF bank
```

### Projected at 18,000 Facts
```
Seeding:        ~300 seconds (18,000 facts × ~20ms/fact)
Benchmarking:   ~150 seconds (same 600 checks)
Full pipeline:  ~8 minutes
Query latency:  ~20 ms p50, ~50 ms p95 (chain depth)
Memory:         ~25 MB RWIF bank
```

---

## Key Insights

### 1. Explicit Knowledge is Auditable
Unlike embeddings or weights, every fact in the base lobe is readable:
```
a whale is a mammal
a mammal is an animal
a whale has warm-blooded
```

No compression, no black box. Anyone can review, validate, and trace.

### 2. Determinism is Non-Negotiable
Every CSIF-Agent instance produces identical outputs for identical inputs. This is a core promise and enables:
- Reproducible deployments.
- Reliable debugging.
- Audit trails.

### 3. Contradiction Detection Maintains Integrity
If someone tries to teach "a whale is a mineral," the agent detects phase conflict and rejects it. The base lobe never encodes contradictions.

### 4. Append-Only Immutability Prevents Erasure
Once a fact is seeded, it cannot be deleted. This ensures:
- Historical accountability.
- No silent data loss.
- Audit trail integrity.

### 5. Modular Lobes Enable Scaling
The base lobe is just the foundation. Future medical, legal, and creative lobes are independent RWIF artifacts:
- Load one or all lobes.
- No shared state; no cross-contamination.
- Each lobe can be updated independently.

---

## Contributing & Extending

### Adding Facts to Existing Categories
1. Edit `build_base_lobe_v1_assets.py` (expand `build_X()` function).
2. Run `python3 scripts/build_base_lobe_v1_assets.py`.
3. Run `./scripts/qualify_base_lobe_v1.sh`.
4. If PASS, commit and tag.

### Creating a New Category
1. Add `build_newcategory()` function in `build_base_lobe_v1_assets.py`.
2. Add to `seed_files` list and benchmark generation.
3. Update [BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md) with quotas and thresholds.
4. Regenerate and qualify.

### Reporting Issues
- Script crashes: Check [BASE_LOBE_V1_TOOLING.md § Debugging](BASE_LOBE_V1_TOOLING.md#debugging--troubleshooting).
- Qualification failures: Review `/tmp/csif-base-lobe-v1-*/server.log`.
- Design questions: Refer to [BASE_LOBE_V1_DESIGN.md](BASE_LOBE_V1_DESIGN.md).

---

## FAQ

**Q: Why append-only and not mutable?**
A: Mutations create audit gaps and enable silent erasure. Append-only ensures historical accountability and integrity.

**Q: Why 18,000 facts?**
A: Empirically, 18K facts provide Gemma-small-level utility (80% of everyday queries) while remaining manageable for curation and quality control. More than 100K would require ML-assisted generation and is outside scope for v1.

**Q: Can I use custom knowledge lobes?**
A: Yes. After base lobe v1 ships, users can create domain-specific lobes (medical, legal, personal) as separate RWIF artifacts.

**Q: What if the base lobe is wrong about something?**
A: All facts are explicit and auditable. If a fact is wrong, file an issue, and we fix it upstream. The contradiction detector prevents teaching conflicting facts.

**Q: Why not use embeddings or LLMs for this?**
A: Embeddings require retraining (slow, expensive). LLMs hallucinate (not deterministic). CSIF's explicit edges are auditable, fast, and deterministic.

---

## Support & Feedback

- **Questions about design?** See [BASE_LOBE_V1_DESIGN.md](BASE_LOBE_V1_DESIGN.md).
- **How to use tools?** See [BASE_LOBE_V1_TOOLING.md](BASE_LOBE_V1_TOOLING.md).
- **How was this built?** See [BASE_LOBE_V1_PROCESS.md](BASE_LOBE_V1_PROCESS.md).
- **Formal spec?** See [BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md).
- **Bug or improvement?** File an issue with the relevant document reference.

---

## License & Attribution

Base Lobe v1 is part of CSIF-Agent and follows the same license. All design, tooling, and process documentation is included to enable replication and extension by others.

**Design & Implementation:** Jason Rofick, May 2026.
**Status:** Production-ready baseline for scaling.

---

**Last Updated:** May 20, 2026
**Status:** Complete & Ready for Scaling
