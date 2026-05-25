# Freeze Release Playbook

This playbook creates a reproducible freeze release with native + Docker parity.

## 1) Run Unified Freeze Gate

From repository root:

```bash
chmod +x scripts/release/freeze_gate.sh
./scripts/release/freeze_gate.sh all
```

Gate output summary is written to:

- `.runtime/native/freeze_gate_latest.txt`

Pass criteria:

- Native: v2 PASS, anti-v3 PASS, math-attacks PASS
- Docker: v2 PASS, anti-v3 PASS, math smoke PASS (lobe-first gate mode uses `CSIF_BOOTSTRAP_BASE_ON_EMPTY=0`)
- Final line: `FREEZE_GATE_VERDICT=PASS`

## 2) Commit Freeze State

```bash
git add lobes/intellect_core/1.0.0/seed/taxonomy.txt \
  scripts/qualify_math_attacks_smoke.sh \
  scripts/release/freeze_gate.sh \
  docs/releases/FREEZE_RELEASE_PLAYBOOK.md \
  docs/releases/FREEZE_v1.0.0-freeze.1.md \
  DOCKER.md

git commit -m "release: freeze gate + lobe baseline parity"
```

## 3) Tag Release

```bash
git tag -a v1.0.0-freeze.1 -m "Freeze release: native+docker parity verified"
git push origin main
git push origin v1.0.0-freeze.1
```

## 4) Build and Push Docker Image

```bash
PUSH=true ./docker-build.sh v1.0.0-freeze.1
```

Optional immutable commit-SHA tag:

```bash
SHA_TAG="$(git rev-parse --short HEAD)"
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t ghcr.io/motechnicalities/csif-agent:v1.0.0-freeze.1 \
  -t ghcr.io/motechnicalities/csif-agent:${SHA_TAG} \
  --push \
  .
```

## 5) Create GitHub Release

Use notes file:

- `docs/releases/FREEZE_v1.0.0-freeze.1.md`

Example with GitHub CLI:

```bash
gh release create v1.0.0-freeze.1 \
  --title "v1.0.0-freeze.1" \
  --notes-file docs/releases/FREEZE_v1.0.0-freeze.1.md \
  --prerelease
```

## 6) Post-Release Validation

Run container from release tag and verify health/query quickly:

```bash
docker run -d --name csif-agent-freeze-check --rm \
  -p 38080:8080 \
  -e CSIF_BOOTSTRAP_BASE_ON_EMPTY=1 \
  -e CSIF_BOOTSTRAP_BASE_MODE=ensure \
  -e CSIF_LOBES_DIR=/app/lobes \
  ghcr.io/motechnicalities/csif-agent:v1.0.0-freeze.1

curl -s http://127.0.0.1:38080/health
curl -s -X POST http://127.0.0.1:38080/query \
  -H "Content-Type: application/json" \
  -d '{"text":"Is a whale a mammal?"}'
```
