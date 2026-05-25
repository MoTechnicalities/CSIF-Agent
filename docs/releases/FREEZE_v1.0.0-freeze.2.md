# CSIF-Agent v1.0.0-freeze.2 (Pre-Release)

Freeze release that corrects old-server baseline drift by aligning Docker Compose defaults with the validated freeze-gate profile.

## Why This Freeze

`v1.0.0-freeze.1` could diverge on old server when using compose defaults that enabled base bootstrap overlay and autonomous mutation loops.

`v1.0.0-freeze.2` switches compose defaults to freeze-safe baseline mode.

## Freeze-Safe Compose Defaults

- `CSIF_BOOTSTRAP_BASE_ON_EMPTY=0`
- `CSIF_BOOTSTRAP_BASE_MODE=empty`
- `CSIF_PLAY_ENABLED=0`
- `CSIF_OBSERVE_ENABLED=0`

These settings keep old-server runtime behavior aligned with freeze-gate qualification.

## Validation

- Unified freeze gate (native + docker): `PASS`
- Native:
  - v2 benchmark: `70/70`
  - anti-v3 benchmark: `25/25`
  - math-attacks qualification: `PASS`
- Docker:
  - v2 benchmark: `70/70`
  - anti-v3 benchmark: `25/25`
  - math smoke: `PASS`

## Notes

- This is still a prerelease checkpoint for controlled rollout.
- If autonomous learning loops are desired, enable them explicitly as an opt-in override, not baseline default.
