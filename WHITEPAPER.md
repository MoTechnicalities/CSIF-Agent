# CSIF-Agent: A Deterministic, Auditable, Multi-Domain Inference Engine

**Version:** 1.4.0  
**Date:** May 19, 2026  
**Author:** Mogir Jason Rofick  
**License:** Apache 2.0  
**Repository:** https://github.com/MoTechnicalities/CSIF-Agent

---

## Abstract

CSIF-Agent is a CPU-native reasoning engine focused on deterministic, auditable inference. It runs without GPU requirements, cloud APIs, or API keys. It rejects unsupported claims with explicit uncertainty states and preserves traceable knowledge evolution through append-only structures.

This whitepaper describes the current architecture, validated capabilities, limitations, and roadmap, including deterministic creativity as an intentional next milestone.

---

## Table of Contents

1. [The Problem](#the-problem)
2. [The Solution](#the-solution)
3. [Architecture](#architecture)
4. [Capabilities](#capabilities)
5. [Validation](#validation)
6. [Deterministic Probability (Creativity)](#deterministic-probability-creativity)
7. [Use Cases](#use-cases)
8. [Performance](#performance)
9. [Comparison to Other Systems](#comparison-to-other-systems)
10. [Limitations](#limitations)
11. [Roadmap](#roadmap)
12. [Getting Started](#getting-started)
13. [Contributing](#contributing)
14. [Citation](#citation)

---

## The Problem

AI tooling is often split between two extremes:

- Probabilistic language systems are flexible but can hallucinate.
- Deterministic systems are reliable but often narrow and brittle.

At the same time, common AI stacks frequently depend on:

- Expensive GPU hardware
- Cloud APIs with recurring cost
- Opaque inference paths with limited auditability

These constraints reduce accessibility for independent developers, small labs, schools, and privacy-sensitive domains.

---

## The Solution

CSIF-Agent is a geometric inference engine built for deterministic behavior and auditable state transitions.

### Core Principles

| Principle | Implementation |
| :--- | :--- |
| Deterministic Truth | Same state + same query -> same answer |
| Auditability | Reasoning is reconstructable from stored edges and trajectories |
| No Hallucination | Unknowns return explicit NEEDS_INPUT responses |
| CPU-Native | Runs locally on commodity hardware |
| Zero Cloud Requirement | No mandatory external API calls |
| Cost Predictability | No mandatory recurring API costs |

---

## Architecture

### Four-Dimensional Substrate

| Dimension | Range | Meaning |
| :--- | :--- | :--- |
| Phase | [-pi, pi) | Semantic alignment (0 coherent, near pi contradictory) |
| Lobe | Discrete | Domain/language separation |
| Time | Continuous | Event trajectory ordering |
| Confidence | [0, pi] | Uncertainty envelope |

### Relation Registry (v1.4.0)

| Relation | Transitive? | Example |
| :--- | :--- | :--- |
| is_a | Yes | whale -> mammal -> animal |
| causes | Yes | rain -> wet ground -> slippery |
| has_property | No | whale has warm-blooded |

### Grammar as Data

Parsing patterns are externalized in grammar.toml and loaded at startup. Adding new language forms no longer requires editing parser code.

### Startup Migration Check

CSIF-Agent stores metadata sidecar information for schema and grammar versions and performs startup normalization/migration checks for legacy bank shapes.

---

## Capabilities

### Deterministic Inference

| Capability | Example Query | Example Response |
| :--- | :--- | :--- |
| Taxonomy lookup | What is a whale? | A whale is a mammal. |
| Transitive is_a | Is a whale an animal? | YES: a whale is an animal. |
| Causal reasoning | Does rain cause slippery? | YES: rain causes slippery. |
| Property query | Does a whale have warm-blooded? | YES: whale has warm-blooded. |
| Negative inference | Does a whale have vertebrate? | NO: cannot establish. |
| Contradiction blocking | a whale is a fish | CONTRADICTION |
| Compute scaffold | What is 2 + 2? | [COMPUTE] 2 + 2 = 4 |

---

## Validation

### Test Environment

- Hardware: CPU-only home server class hardware
- OS: Linux
- Deployment: Docker container

### Regression Snapshot (v1.4.0)

| Test | Query | Response |
| :--- | :--- | :--- |
| Health | /health | ok |
| Direct Query | What is a whale? | A whale is a mammal. |
| Teaching | a whale is a mammal | TEACHING |
| Chain Teaching | a mammal is an animal | TEACHING |
| Transitive Inference | Is a whale an animal? | YES: a whale is an animal. |
| Causal Chain | rain causes wet ground; wet ground causes slippery | TEACHING |
| Causal Inference | Does rain cause slippery? | YES: rain causes slippery. |
| Property Teaching | a whale has warm-blooded | TEACHING |
| Property Query | Does a whale have warm-blooded? | YES: whale has warm-blooded. |
| Negative Property | Does a whale have vertebrate? | NO: cannot establish. |
| Arithmetic Scaffold | What is 2 + 2? | [COMPUTE] 2 + 2 = 4 |
| Contradiction | a whale is a fish | CONTRADICTION |

---

## Deterministic Probability (Creativity)

Deterministic creativity is an explicit design target. The intended model is temporal phase evolution with reproducible variation:

```text
theta(t) = wrap_pi(theta_0 + sigma * sin(0.618 * t))
```

Where:

- t = explicit time coordinate
- sigma = configurable creativity amplitude
- 0.618 = golden-ratio coefficient for deterministic variation

Status in v1.4.0:

- Compute and relation inference are active.
- Reproducible creative generation is planned (roadmap section).

---

## Use Cases

### Science and Research

Teach causal and categorical relations and ask deterministic chain questions with auditable outputs.

### Education

Run locally in classrooms or labs and enforce explicit unknown handling instead of fabricated answers.

### Legal and Compliance Workflows

Represent rule relationships and test deterministic implications with reconstructable reasoning paths.

### Engineering and Safety

Encode requirement dependencies and test whether downstream implications hold.

### Personal Knowledge Systems

Maintain a local, append-only memory graph with deterministic query behavior.

---

## Performance

Representative project benchmarks and observed behavior emphasize low-latency CPU-native operation.

| Operation | Throughput | Latency |
| :--- | :--- | :--- |
| Phase operations | ~99M ops/sec | ~10 ns |
| Edge resonance scan | ~520M edges/sec | sub-ms at small/medium bank sizes |
| Agent cache hit | - | < 1 ms |
| Contradiction check | - | < 10 us |
| Short transitive chain | - | typically ms-scale |

---

## Comparison to Other Systems

| System Class | Deterministic | Auditable | CPU-Only Friendly | Hallucination-Free by Default |
| :--- | :---: | :---: | :---: | :---: |
| CSIF-Agent | Yes | Yes | Yes | Yes |
| Typical LLM API flows | No | Limited | No | No |
| Local LLM-only flows | No | Limited | Often GPU-bound | No |
| Rule engines | Yes | Yes | Yes | Yes |

---

## Limitations

| Limitation | Current Status | Target |
| :--- | :--- | :--- |
| Open-domain NL understanding | Limited grammar-driven parsing | Expand grammar + adapters |
| Arithmetic breadth | Addition scaffold only | Expanded operators/algebra |
| Relation set breadth | is_a, causes, has_property | Additional relation families |
| Large-scale bank characterization | In progress | Dedicated scale validation |
| Creative generation | Planned | Deterministic creative mode |

---

## Roadmap

| Version | Focus |
| :--- | :--- |
| v1.4.0 | Multi-domain inference: taxonomy, causality, property relations + compute scaffold |
| v1.5 | Expanded arithmetic and relation-aware inheritance experiments |
| v1.6 | Large crystal-bank scale profiling and validation |
| v1.7 | Deterministic creativity output layer |
| v2.0 | Broader external validation and hardened production guidance |

---

## Getting Started

### Docker (Recommended)

```bash
docker run -d --name csif-agent --restart unless-stopped \
  -p 8080:8080 \
  -v csif-agent-data:/data \
  ghcr.io/motechnicalities/csif-agent:latest
```

### Teach

```bash
curl -X POST http://localhost:8080/teach \
  -H "Content-Type: application/json" \
  -d '{"text":"a whale is a mammal"}'
```

### Query

```bash
curl -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"text":"What is a whale?"}'
```

---

## Contributing

Contributions are welcome in:

- New relation families
- Expanded mathematical capabilities
- Deterministic creative layer
- Performance optimization
- Documentation and validation suites

Contribution guardrails:

- Determinism: same input and state -> same output
- Auditability: preserve traceability and provenance
- Local-first operation: no mandatory GPU/cloud dependency

---

## Citation

```bibtex
@software{CSIF_Agent_2026,
  author = {Mogir Jason Rofick},
  title = {CSIF-Agent: A Deterministic, Auditable, Multi-Domain Inference Engine},
  year = {2026},
  url = {https://github.com/MoTechnicalities/CSIF-Agent},
  note = {Version 1.4.0},
  license = {Apache-2.0}
}
```

---

## Conclusion

CSIF-Agent demonstrates that deterministic, auditable, local inference can be practical on commodity hardware while still evolving toward broader reasoning domains and deterministic creativity.

The core direction is clear: systems should explicitly know what they know, explicitly acknowledge what they do not, and never fabricate certainty.
