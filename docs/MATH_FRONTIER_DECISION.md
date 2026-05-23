# Math Frontier Decision

**Date:** May 22, 2026
**Status:** Adopted

## Decision

For the new math surface, the implementation strategy is:

1. Prefer exact, bounded algebra over general symbolic search.
2. Represent results canonically with exact rational arithmetic and interval algebra.
3. Classify unsupported forms explicitly instead of approximating or guessing.
4. Validate the live service with a reproducible smoke gate after every substantive change.

## Why This Method

This repo is in new-frontier territory. The useful move is not to brute-force a universal CAS. The useful move is to keep the problem inside a deterministic envelope where the runtime can prove what it knows.

That means:

- linear equations and systems remain exact and finite
- quadratic equations and inequalities stay in exact-rational form when possible
- rational inequalities are handled by sign analysis and excluded points
- interval unions, intersections, complements, and holes are normalized canonically
- higher-dimensional linear systems are solved with elimination and consistency classification

This gives the best tradeoff for an interactive agent:

- predictable outputs
- small computational cost
- auditable derivations
- clean failure modes for unsupported inputs

## What We Intentionally Did Not Choose

- No brute-force universal symbolic solver
- No approximate numerical fallback for core solve mode
- No hidden search over broad algebraic forms
- No GPU dependency for the current math layer

Those approaches either become unstable, expensive, or opaque. They are not the right default for this codebase.

## Validation Standard

The chosen method is only considered valid when it passes both of these checks:

1. `cargo test -p csif-agent`
2. `bash scripts/qualify_math_attacks_smoke.sh`

The smoke gate is important because it exercises the live HTTP service, not just the library layer.

## Current Frontier Coverage

Implemented and validated:

- exact-rational linear solving
- quadratic solving
- quadratic inequalities
- rational inequalities with exclusion points
- interval normalization, union, intersection, difference, complement, and hole rendering
- square NxN linear systems with consistency classification

## Guiding Rule Going Forward

If a new math feature can be made exact, finite, and deterministic, implement it in that form first.

If it cannot be made exact without exploding scope, return an explicit unsupported result instead of inventing a guess.