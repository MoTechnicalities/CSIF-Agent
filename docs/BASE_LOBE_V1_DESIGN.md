# Base Lobe v1 Design Document

## Executive Summary

Base Lobe v1 is the inaugural knowledge substrate for CSIF-Agent—a curated, quality-first collection of semantic facts designed to ship with every CSIF-Agent instance, providing day-one practical utility comparable to a small foundation model without hallucination or forgetting.

**Core Design Philosophy:** Explicit > Implicit. Auditable > Opaque. Deterministic > Probabilistic.

---

## Problem Statement

### Traditional Approach (LLMs)
- Monolithic weight tensors encode billions of facts as compressed probability distributions.
- Cannot add knowledge without retraining (hours, expensive, forgetting).
- Hallucination rate 5-10% (well-documented).
- No audit trail; no contradiction detection.
- Single failure mode: if something goes wrong, the entire model is compromised.

### CSIF-Agent's Opportunity
- Append-only trajectory log; never overwrites or forgets.
- Contradiction detection via geometric phase conflict ($\theta \approx \pi$).
- Query-time reasoning over explicit edges, not dense inference.
- Modular lobes: one brain, many domain packages.

### What Base Lobe v1 Solves
Users should not start with a blank slate. Base Lobe v1 provides:
- Common taxonomies (animals, plants, objects, vehicles, devices).
- Everyday causality (rain → wet, fire → smoke).
- Basic properties (whale → warm-blooded).
- Geography (cities, countries, continents).
- Arithmetic (addition, basic algebra).
- Operator utility (system concepts for internal workflows).

This enables CSIF-Agent to answer 80% of everyday deterministic factual queries out-of-the-box, with full transparency and zero hallucination.

---

## Design Constraints

### 1. Quality Over Breadth
- **Target:** Gemma-small utility equivalent (not breadth, not LLM fluency).
- **Metric:** High pass rate on common-sense questions, zero false positives on unknowns.
- **Not:** Open-domain conversational breadth; niche knowledge; probabilistic patterns.

### 2. Ship-Ready Quotas
Total seeded facts: **18,000** (target across all categories).

| Category | Quota | Reasoning |
|---|---:|---|
| Taxonomy | 8,000 | 50K words = ~2-3 hypernym chains per word |
| Causality | 2,500 | Common causal chains (cause → effect → effect). |
| Properties | 3,000 | Attributes of common objects/animals. |
| Geography | 2,000 | Cities, countries, continents, major locations. |
| Arithmetic | 1,500 | Addition, multiplication tables, algebraic identities. |
| Operator Utility | 1,000 | System/workflow concepts (endpoint, contradiction, etc.). |

### 3. Append-Only Immutability
- Once a fact is seeded, it is never deleted or overwritten.
- New evidence appends; old evidence remains for audit.
- Contradiction detection blocks inconsistent teaches at write-time.

### 4. Deterministic Replay
- Same bank + same query = same answer, every run, every platform.
- No randomness; no stochastic sampling.
- Full audit trail of reasoning (transitive chains, phase signatures).

### 5. Zero External Dependencies
- Script tooling uses only bash and Python standard library.
- No LLM calls during seeding (contrast: RAG pipelines often call GPT to curate).
- No cloud services; offline-first operation.

---

## Architectural Decisions

### 1. Seed Data Organization

```
data/base_lobe_v1/
  seed/
    taxonomy.txt        # "a whale is a mammal"
    causality.txt       # "rain causes wet ground"
    properties.txt      # "a whale has warm-blooded"
    geography.txt       # "a paris is a city"
    arithmetic.txt      # "1 + 1 = 2"
    operator_utility.txt
  benchmarks/
    base_lobe_v1_benchmark.jsonl  # 600 test cases
```

**Rationale:**
- Plain text for human readability and version control.
- JSONL benchmark for structured test semantics.
- Separation of seeding assets from qualification harness.

### 2. Build-Time vs. Runtime Generation

**Build-time:**
- `build_base_lobe_v1_assets.py` generates seed data and benchmark from programmatic definitions.
- Ensures reproducible, testable asset generation.
- Allows scaling without manual data curation.

**Runtime:**
- `seed_base_lobe_v1.sh` teaches facts to a running agent instance.
- `run_base_lobe_v1_benchmark.py` validates correctness against a live bank.
- `qualify_base_lobe_v1.sh` orchestrates full end-to-end qualification in isolation.

**Rationale:**
- Decouples asset creation from seeding operation.
- Allows testing, validation, and iterative scaling independently.
- Build artifacts are version-controlled; runtime executions are ephemeral.

### 3. Isolation via Temporary Banks

Qualification runs in an isolated temporary bank (`/tmp/csif-base-lobe-v1-XXXXXX/`) with:
- Separate server instance on a unique port.
- No interference with production or developer banks.
- Automatic cleanup on completion (trap EXIT).

**Rationale:**
- Prevents qualification from polluting real data.
- Enables concurrent qualification runs if needed.
- Artifact preservation for audit/debugging.

### 4. Contradiction-Safe Seeding

Each teach is checked against the existing bank state:
- **Phase proximity:** If incoming fact aligns at $\theta \approx 0$, accepted.
- **Phase conflict:** If incoming fact opposes at $\theta \approx \pi$, rejected (logged, not taught).
- **Transitive validation:** If a contradiction would create a logical cycle, blocked.

**Rationale:**
- Prevents base lobe from encoding inconsistent propositions.
- Catches hand-curated data errors before they enter the log.
- Maintains integrity guarantee: every fact in the bank is self-consistent.

### 5. Benchmark Structure: 600 Test Cases

Distributed across 7 categories:

| Category | Count | Purpose |
|---|---:|---|
| Taxonomy (direct) | 120 | "What is a whale?" |
| Taxonomy (transitive) | 120 | "Is a whale an animal?" (2+ hops). |
| Properties | 165 | "Does a whale have warm-blooded?" |
| Causality | 45 | "Does rain cause slippery?" |
| Arithmetic | 60 | "What is 2 + 2?" |
| Contradiction | 45 | Reject "a whale is a mineral" (has CONTRADICTION in response). |
| Honesty | 45 | Reject unknowns ("What is unknown_entity_001?"). |

**Rationale:**
- Transitive reasoning (2+ hops) validates chain integrity.
- Contradiction checks ensure reject-case quality.
- Honesty checks ensure agent doesn't fabricate answers.
- 600 cases provide statistical significance without excessive runtime.

### 6. Pass/Fail Gates

All thresholds must be met for ship-ready status:

| Gate | Threshold | Consequence |
|---|---|---|
| Direct accuracy | >= 92% | Taxonomy questions answered correctly. |
| Transitive accuracy | >= 95% | Chain reasoning works reliably. |
| Property accuracy | >= 90% | Attribute lookups succeed. |
| Arithmetic accuracy | >= 98% | Computation is correct. |
| Contradiction rejection | >= 99% | Rejects false teaches reliably. |
| Honesty (false positive) | <= 1% | Never invents answers. |
| Determinism replay | 100% | Bit-identical across 3 runs. |
| Latency p50 | <= 10 ms | Acceptable local performance. |
| Latency p95 | <= 25 ms | No pathological slowdowns. |

**Rationale:**
- High thresholds reflect the "zero hallucination" promise.
- Transitive >= 95% > direct >= 92% (chains are harder, must be more reliable).
- Arithmetic >= 98% (math is non-negotiable).
- Contradiction >= 99% (integrity is non-negotiable).
- Determinism = 100% (not a tolerance; it's a requirement).

---

## Scalability Considerations

### Current v1 State (May 20, 2026)
- Seeded: **713 facts** (4.0% of 18K quota).
- Qualified: **600 benchmark checks** (PASS: 600/600, 100%).
- Latency: < 10 ms p50 (local CPU).

### Scaling Path to 18K Facts
1. **Milestone 1 (3K facts):** Expand taxonomy and properties; validate gates.
2. **Milestone 2 (8K facts):** Add geography and causality depth.
3. **Milestone 3 (15K facts):** Expand arithmetic; refine operator utility.
4. **Milestone 4 (18K facts):** Final validation; prepare v1 release.

### Expansion Strategy
- Use WordNet, ConceptNet, Wikidata for source data.
- Programmatic extraction in `build_base_lobe_v1_assets.py`.
- Contradiction checks during seeding catch inconsistencies automatically.
- Benchmark adapted dynamically as categories grow.

### Memory/Performance Expectations
- **Current:** 713 facts → ~1 MB uncompressed, < 10 ms per query (CPU-local).
- **At 18K facts:** ~25 MB uncompressed, ~20-50 ms per query (depending on chain depth).
- **Hardware:** Works on 12-year-old hardware; no GPU required.

---

## Future Lobes (After v1)

Once Base Lobe v1 ships, domain-specific lobes extend capability:

| Lobe | Size | Use Cases |
|---|---|---|
| Medical | 500K facts | Clinical decision support, disease definitions. |
| Legal | 300K facts | Statute references, precedent chains. |
| Programming | 200K facts | API docs, language semantics, library facts. |
| Creative | 100K facts | Poetry metaphors, analogies, narrative patterns. |
| Custom (user) | Unlimited | Business rules, personal knowledge graphs. |

Each lobe is:
- Optional (download separate).
- Independent (no shared state with other lobes).
- Queryable by name or in federated mode (query all, aggregate results).

---

## Design Trade-offs

| Aspect | Choice | Why Not Alternative |
|---|---|---|
| Explicit edges, not embeddings | CSIF edges (deterministic) | Embeddings require retraining, don't audit. |
| Append-only, not mutable | Immutable log | Mutations create audit gaps, enable erasure. |
| Teach-time validation, not post-hoc filtering | Reject at source | Garbage in + filtering = still messy data. |
| Small v1, not comprehensive | 18K facts | 18K is maintainable; 1M is unmaintainable without ML. |
| Local CPU, not cloud/GPU | Zero external deps | Cloud adds latency, cost, trust surface. |
| Plain text seeds, not binary blobs | Readable sources | Version control, human review, reproducibility. |

---

## Success Metrics (v1 Ready for Ship)

1. **Correctness:** 600/600 benchmark checks pass.
2. **Quota:** 18,000 facts seeded (or justified subset).
3. **Gates:** All 8 gates pass (accuracy, latency, determinism, honesty).
4. **Reproducibility:** Qualification produces identical JSON score across 3 independent runs.
5. **Documentation:** This document + tooling runbook complete.
6. **No Regressions:** All CSIF-Agent existing demos still work with base lobe loaded.

---

## References

- [BASE_LOBE_V1_SPEC.md](BASE_LOBE_V1_SPEC.md) — Formal specification (quotas, thresholds, gates).
- [BASE_LOBE_V1_TOOLING.md](BASE_LOBE_V1_TOOLING.md) — Script reference and operations guide.
- [BASE_LOBE_V1_PROCESS.md](BASE_LOBE_V1_PROCESS.md) — Step-by-step process and examples.
- [CSIF](../CSIF/) — Core semantic engine documentation.
- [RWIF](../RWIF/) — Append-only fact storage documentation.
