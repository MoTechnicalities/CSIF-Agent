## CSIF-Agent: Geometric Intelligence for CPU-Native Agents

[![Rust](https://img.shields.io/badge/rust-1.80+-blue.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-green.svg)](LICENSE)
[![Benchmarks](https://img.shields.io/badge/benchmarks-99M%20ops%2Fs-brightgreen)](docs/BENCHMARKS.md)
[![Stars](https://img.shields.io/github/stars/MoTechnicalities/CSIF-Agent?style=social)](https://github.com/MoTechnicalities/CSIF-Agent)

> **Deterministic, auditable, CPU-native intelligence. No GPU. No cloud. No hallucination.**

## Read the Whitepaper

For a comprehensive overview of CSIF-Agent architecture, capabilities, validation, and roadmap, see [WHITEPAPER.md](WHITEPAPER.md).

## Docker (Recommended)

Run the agent in seconds with no Rust toolchain:

```bash
docker run -d --name csif-agent --restart unless-stopped \
	-p 8080:8080 \
	-e CSIF_BANK_PATH=/data/my_brain.rwif \
	-e CSIF_GRAMMAR_PATH=/app/grammar.toml \
	-v csif-agent-data:/data \
	ghcr.io/motechnicalities/csif-agent:latest
```

### Docker Compose

```bash
docker compose up -d
```

### Pull From GHCR

```bash
docker pull ghcr.io/motechnicalities/csif-agent:latest
```

### Multi-Arch Support

- `linux/amd64` (Intel/AMD)
- `linux/arm64` (Apple Silicon, ARM servers, Raspberry Pi)

### Persistence

Knowledge is stored in `/data/my_brain.rwif` inside the container. Mount `/data` to a named volume or host path so memory survives restarts.

### Upgrade

```bash
docker pull ghcr.io/motechnicalities/csif-agent:latest
docker stop csif-agent && docker rm csif-agent
docker run -d --name csif-agent --restart unless-stopped \
	-p 8080:8080 \
	-e CSIF_BANK_PATH=/data/my_brain.rwif \
	-e CSIF_GRAMMAR_PATH=/app/grammar.toml \
	-v csif-agent-data:/data \
	ghcr.io/motechnicalities/csif-agent:latest
```

## The Problem

Every AI agent today pays a hidden tax: cloud APIs, GPU clusters, and probabilistic hallucinations. OpenClaw + Gemini costs real money. Local LLMs require 24GB+ VRAM. Vector databases add latency and opacity.

**There is no local, deterministic, auditable agent brain that runs on CPU — until now.**

## The Solution

CSIF-Agent implements **phase geometry** over a four-dimensional knowledge substrate:

- **Phase** = semantic alignment (0 = coherent, π = contradiction)
- **RWIF** = append-only trajectories (full auditability)
- **Guard** = multi-path contradiction detection
- **Sync** = SKIP/NUDGE/REJECT consensus protocol
- **Cache** = phase-resonant preflight routing

**The result:** a CPU-native agent that learns, remembers, rejects contradictions, and responds in milliseconds — all on your existing hardware.

## Performance (Rust Native)

| Operation | Throughput | Latency |
| :--- | :--- | :--- |
| Phase operations | 99M ops/sec | ~10 ns |
| Edge resonance scan | 520M edges/sec | sub-ms for 10K bank |
| Agent cache hit | — | < 1 ms |
| Contradiction check | — | < 10 µs |

## Rust Quick Start (Alternative)

```bash
git clone https://github.com/MoTechnicalities/CSIF-Agent
cd CSIF-Agent
cargo build --release
./target/release/agent_demo
```

### Data-Driven Grammar

Parsing rules are loaded from `grammar.toml` at startup. You can add new phrasing patterns by editing this file and restarting the agent, without recompiling or wiping the crystal bank.

```bash
CSIF_GRAMMAR_PATH=./grammar.toml ./target/release/agent_demo
```

```bash
# Teach the agent
curl -X POST http://localhost:8080/teach -H "Content-Type: application/json" -d '{"text":"A whale is a mammal."}'

# Query the agent
curl -X POST http://localhost:8080/query -H "Content-Type: application/json" -d '{"text":"What is a whale?"}'

# Contradiction is rejected automatically
curl -X POST http://localhost:8080/teach -H "Content-Type: application/json" -d '{"text":"A whale is a fish."}'
# Returns: [CONTRADICTION] That contradicts what I already know.
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     CSIF-Agent (HTTP)                   │
├──────────────┬──────────────┬──────────────┬───────────┤
│  CSIF-Cache  │  CSIF-Guard  │  CSIF-Sync   │  RWIF     │
│  (preflight) │ (contradict) │ (consensus)  │ (storage) │
└──────────────┴──────────────┴──────────────┴───────────┘
															│
															▼
										Pure CPU, no GPU, no cloud
```

## Validation (All Pass)
| Test | Expected | Actual |
|---|---|---|
| Query (empty) | "I don't know" | ✅ |
| Teach true fact | "Knowledge crystallized" | ✅ |
| Query after teach | Returns fact | ✅ |
| Teach contradiction | "CONTRADICTION" | ✅ |
| Query after contradiction | Still true fact | ✅ |
| Persistence (restart) | Remembers fact | ✅ |

## Comparison
| System | GPU? | Cloud? | Contradiction Detection | Audit Trail | Cost |
|---|---|---|---|---|---|
| OpenClaw + Gemini | No | Yes | ❌ | ❌ | $$$ |
| Local LLM (7B+) | 24GB+ | No | ❌ | ❌ | Hardware |
| Vector DB + GPT | No | Yes | ❌ | ❌ | $$ |
| CSIF-Agent | No | No | ✅ π residual | ✅ RWIF | $0 |

## Roadmap
- v1.1 SIMD acceleration (AVX-512) → 4B edges/sec
- v1.2 GPU offload for phase scans → 1T edges/sec
- v1.3 ESP-32 deployment → $5 swarm agents
- v2.0 Custom ASIC → picowatt per query

## Honest Limitations
- Concept extraction from natural language is minimal (requires structured input or tiny LLM translator)
- Scale validation at 10K+ crystals not yet characterized
- Multi-hop reasoning currently supports transitive `is_a` chains; broader relation inference is planned

## Contributing
See CONTRIBUTING.md. All contributions must maintain:
- Determinism (same input → same output)
- Auditability (full provenance traces)
- No GPU or cloud dependencies

## License
Apache 2.0. See LICENSE.

## Citation

```bibtex
@software{CSIF_Rust_Trio_2026,
	author = {Mogir Jason Rofick},
	title = {CSIF-Agent: Geometric Intelligence for CPU-Native Agents},
	year = {2026},
	url = {https://github.com/MoTechnicalities/CSIF-Agent},
	license = {Apache-2.0}
}
```

The age of deterministic, auditable, CPU-native intelligence has begun.
