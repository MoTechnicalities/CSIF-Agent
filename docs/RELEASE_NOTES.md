# CSIF-Agent v1.0.0 Release Notes

**Release Date:** May 18, 2026  
**License:** Apache 2.0  
**Repository:** https://github.com/MoTechnicalities/CSIF-Agent

## Summary

CSIF-Agent is the first production-ready, CPU-native, deterministic intelligence engine that runs entirely on local hardware with no GPU, no cloud, and no API costs. It uses **phase geometry** to represent knowledge, detect contradictions (pi residual), and persist auditable trajectories (RWIF format).

## Performance Benchmarks

| Operation | Throughput | Latency |
| :--- | :--- | :--- |
| Phase operations (Rust) | 99M ops/sec | ~10 ns |
| Edge resonance scan | 520M edges/sec | sub-ms for 10K bank |
| Agent cache hit | - | < 1 ms |
| Contradiction check | - | < 10 us |

## Validation Results (All Pass)

| Test | Expected Response | Actual |
| :--- | :--- | :--- |
| Query (empty knowledge) | `[NEEDS_INPUT]` | ✅ |
| Teach true fact | `[TEACHING] Knowledge crystallized.` | ✅ |
| Query after teach | Returns taught fact | ✅ |
| Teach contradiction | `[CONTRADICTION]` | ✅ |
| Query after contradiction | Still returns true fact | ✅ |
| Persistence (restart) | Remembers taught fact | ✅ |

## What's Included

- **CSIF-Core**: Phase math (`wrap_pi`, `resonance`, `contradiction_threshold`)
- **RWIF-Core**: Append-only trajectory persistence (JSON)
- **CSIF-Guard**: Multi-path contradiction detection
- **CSIF-Sync**: SKIP/NUDGE/REJECT consensus protocol
- **CSIF-Cache**: Phase-resonant preflight routing
- **CSIF-Agent**: HTTP server with teach/query endpoints

## Quick Start

```bash
git clone https://github.com/MoTechnicalities/CSIF-Agent
cd CSIF-Agent
cargo build --release
./target/release/agent_demo

# In another terminal:
curl -X POST http://localhost:8080/teach -H "Content-Type: application/json" -d '{"text":"A whale is a mammal."}'
curl -X POST http://localhost:8080/query -H "Content-Type: application/json" -d '{"text":"What is a whale?"}'
```

## What's Next (Roadmap)

v1.1: SIMD acceleration (AVX-512) -> 4B edges/sec

v1.2: GPU offload for phase scans -> 1T edges/sec

v1.3: ESP-32 deployment -> $5 swarm agents

v2.0: Custom ASIC -> picowatt per query

## Known Limitations (Honest Disclosure)

Concept extraction from natural language is minimal (requires structured input or tiny LLM translator)

Scale validation at 10K+ crystals not yet characterized

No multi-hop reasoning beyond direct edge lookup (planned for v1.2)

## Contributing

See CONTRIBUTING.md. All contributions must maintain:

Determinism (same input -> same output across platforms)

Auditability (full provenance traces)

Zero GPU or cloud dependencies

## Citation

```bibtex
@software{CSIF_Agent_2026,
  author = {Mogir Jason Rofick},
  title = {CSIF-Agent: Geometric Intelligence Engine},
  year = {2026},
  url = {https://github.com/MoTechnicalities/CSIF-Agent},
  license = {Apache-2.0}
}
```

The age of deterministic, auditable, CPU-native intelligence has begun.

---

## v1.0.1 Ship Hardening Addendum (May 21, 2026)

This addendum captures release-critical hardening completed after the original v1.0.0 notes.

### 1. Modular Lobe Admin Endpoints

Added two admin endpoints for lobe observability and manual control:

- `GET /admin/lobes`: list configured lobe runtime settings and currently applied bundle fingerprints.
- `POST /admin/lobes/reload`: force a refresh from `CSIF_LOBES_DIR` and return a refresh report.

### 2. Optional Admin Auth Guard

Admin routes are optionally protected by `CSIF_ADMIN_TOKEN`.

- If `CSIF_ADMIN_TOKEN` is unset: admin routes are open (default compatibility mode).
- If `CSIF_ADMIN_TOKEN` is set: requests must include either:
  - `X-CSIF-Admin-Token: <token>`
  - `Authorization: Bearer <token>`

Failed/missing auth returns HTTP `401` with JSON error payload.

### 3. Elaborated Describe Responses

`What is ...?` responses now include more than a direct class when data exists:

- base classification (`A whale is a mammal.`)
- optional property phrase (`It can be warm-blooded, aquatic, and large.`)
- optional subtype examples (`There are several types, including blue whale, killer whale, and sperm whale.`)

This improves user-facing quality while preserving deterministic, fact-driven generation.

### 4. Crystallized Response Templates (Data-Driven)

Describe wording moved to grammar configuration (`grammar.toml`) under `[templates.describe]`.

This enables response tone/list-style changes without code edits to Rust formatting logic.

Configurable fields:

- `classification`
- `properties_intro`, `properties_outro`, `property_connector`
- `subtypes_intro`, `subtypes_outro`, `subtype_connector`
- `oxford_comma`
- `max_subtype_examples`

### 5. Verification Snapshot

Validated on live runs in local runtime:

- compile checks pass: `cargo check -p csif-agent -p agent_demo`
- admin auth behavior: `200` open mode, `401` unauthenticated token mode, `200` authenticated token mode
- describe customization behavior: editing template text in grammar changes runtime answer wording as expected

### 6. Compatibility Notes

- Existing query and teach APIs are unchanged.
- Existing grammar files without `[templates.describe]` remain valid (safe defaults applied).
- Existing lobe bundles remain compatible.