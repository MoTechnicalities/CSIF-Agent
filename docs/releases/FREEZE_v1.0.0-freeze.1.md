# CSIF-Agent v1.0.0-freeze.1 (Pre-Release)

Freeze release focused on deterministic baseline parity across native and Docker execution.

## Freeze Gate Verdict

- Unified gate: `FREEZE_GATE_VERDICT=PASS`
- Native gate:
  - v2 benchmark: `70/70`
  - anti-v3 benchmark: `25/25`
  - math-attacks qualification: `PASS`
- Docker gate:
  - v2 benchmark: `70/70`
  - anti-v3 benchmark: `25/25`
  - math smoke: `PASS`

## Included Freeze Fixes

- Core lobe taxonomy repair for animal hierarchy:
  - `a whale is a mammal`
  - `a mammal is a animal`
- Native startup defaults include lobe loading via `CSIF_LOBES_DIR`.
- Unified freeze-gate script for native + Docker parity.

## Docker Baseline

- Image includes lobe bundles under `/app/lobes`.
- Runtime default `CSIF_LOBES_DIR=/app/lobes`.
- Base bootstrap defaults:
  - `CSIF_BOOTSTRAP_BASE_ON_EMPTY=1`
  - `CSIF_BOOTSTRAP_BASE_MODE=ensure`

## Notes

- This is a freeze checkpoint for validation and reproducibility.
- Promote to full stable once downstream 31B integration and long-run soak checks complete.
