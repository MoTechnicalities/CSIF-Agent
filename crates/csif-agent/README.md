# csif-agent

Deterministic, phase-resonant agent framework for the CSIF trilogy (Rust).

## Purpose

This crate orchestrates the trilogy (csif-guard, csif-sync, csif-cache) into a unified agent loop:
- Ingests and validates input
- Caches and routes queries
- Guards against contradictions
- Syncs state across nodes
- Persists all actions (RWIF)

## Status
- [x] Scaffolded crate, config, and agent loop stub
- [ ] Implement agent loop logic
- [ ] Add HTTP/UDP interface
- [ ] Integration tests

## License
Apache 2.0
