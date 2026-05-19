#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-v1.1.0}"
IMAGE="${IMAGE:-ghcr.io/motechnicalities/csif-agent}"
PLATFORMS="linux/amd64,linux/arm64"
PUSH_FLAG="${PUSH:-false}"

if ! docker buildx inspect >/dev/null 2>&1; then
  docker buildx create --use >/dev/null
fi

TAGS=(
  "-t" "${IMAGE}:latest"
  "-t" "${IMAGE}:${VERSION}"
)

if [[ "${PUSH_FLAG}" == "true" ]]; then
  echo "Building and pushing ${IMAGE}:latest and ${IMAGE}:${VERSION} for ${PLATFORMS}"
  docker buildx build \
    --platform "${PLATFORMS}" \
    "${TAGS[@]}" \
    --push \
    .
else
  echo "Building multi-arch image locally for ${PLATFORMS} (no push)"
  docker buildx build \
    --platform "${PLATFORMS}" \
    "${TAGS[@]}" \
    --load \
    .
  echo "Local image available as ${IMAGE}:latest"
fi

cat <<EOF
Done.
To push to GHCR:
  PUSH=true ./docker-build.sh ${VERSION}
EOF
