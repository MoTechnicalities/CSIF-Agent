# Base Lobe v1 Spec (Ship-Ready)

Version: 1.0
Date: 2026-05-20
Status: Draft for implementation

## 1) Product Goal

Ship CSIF-Agent with a bundled base knowledge lobe that delivers practical day-one utility comparable to a small local model experience for common factual tasks, while preserving deterministic and auditable behavior.

Non-goal: open-domain conversational breadth.

## 2) Exact Fact Category Quotas

Target total: 18,000 accepted facts in the base lobe.

| Category | Relation Focus | Quota |
|---|---|---:|
| Core taxonomy | `is_a` (transitive) | 8,000 |
| Common causality | `causes` (transitive) | 2,500 |
| Common properties | `has_property` (direct) | 3,000 |
| Geography/location | `is_a` + location phrasing mapped to supported relations | 2,000 |
| Arithmetic identities/facts | compute coverage + teachable helper facts | 1,500 |
| Operator utility pack | command, workflow, infra baseline facts | 1,000 |

Quota acceptance rule: facts count only after normalization and contradiction check pass.

## 3) Data Quality Rules

1. Facts must be canonicalized (articles removed, normalized casing).
2. Duplicate triples are dropped at ingest.
3. Contradictory teaches are rejected and logged.
4. Source provenance is retained externally in build metadata.
5. Controversial claims are excluded from base lobe and moved to optional domain lobes.

## 4) Benchmark Question Set Structure

Canonical benchmark file format: JSONL

Each line schema:

```json
{"id":"taxo-0001","type":"query","query":"What is a whale?","expected_mode":"contains","expected":"mammal","category":"taxonomy"}
```

Required benchmark composition (600 total checks):

- 240 direct retrieval checks
- 120 transitive inference checks
- 90 causal/property checks
- 60 arithmetic checks
- 45 contradiction rejection checks (teach attempts)
- 45 honesty checks (out-of-scope, must return negative/needs-input)

## 5) Pass/Fail Thresholds

All thresholds are mandatory for ship-ready.

1. Direct retrieval accuracy: >= 92%
2. Transitive inference accuracy: >= 95%
3. Causal/property accuracy: >= 90%
4. Arithmetic accuracy: >= 98%
5. Contradiction rejection rate: >= 99%
6. Honesty guard (false positive on unknown): <= 1%
7. Determinism replay (same bank/query over 3 runs): 100%
8. Performance (local Docker target profile):
   - p50 query latency <= 10 ms
   - p95 query latency <= 25 ms

## 6) Release Gates (Ship-Ready)

Gate A: Data Build Gate
- Quotas met per category.
- Ingest logs show zero unresolved parser failures.

Gate B: Correctness Gate
- Full 600-check benchmark passes thresholds.

Gate C: Stability Gate
- Determinism replay hash identical across 3 consecutive runs.

Gate D: Runtime Gate
- Latency thresholds met on target machine profile.

Gate E: Packaging Gate
- Base lobe bundled in image and documented.
- Optional lobe mount path documented.

Gate F: Documentation Gate
- README references base lobe behavior and limitations.
- OPENAI compatibility examples remain host-template safe.

## 7) Implementation Plan (v1 Kickoff)

Phase 1: Scaffolding (this commit scope)
- Seed file layout per category.
- Seeder script and benchmark runner.
- Initial small benchmark sample for smoke testing.

Phase 2: Data Expansion
- Fill category files to quota targets.
- Add provenance manifest and ingest logs.

Phase 3: Qualification
- Run full benchmark set and publish metrics.
- Freeze base lobe artifact and tag release.

## 8) Artifact Layout

```text
data/base_lobe_v1/
  seed/
    taxonomy.txt
    causality.txt
    properties.txt
    geography.txt
    arithmetic.txt
    operator_utility.txt
  benchmarks/
    base_lobe_v1_benchmark.jsonl
scripts/
  seed_base_lobe_v1.sh
  run_base_lobe_v1_benchmark.py
```
