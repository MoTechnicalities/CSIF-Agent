#!/usr/bin/env bash
set -euo pipefail

IMAGE="${IMAGE:-ghcr.io/motechnicalities/csif-agent:latest}"
CONTAINER_NAME="${CONTAINER_NAME:-csif-agent}"
DATA_VOLUME="${DATA_VOLUME:-csif-agent-data}"
HOST_PORT="${HOST_PORT:-8080}"
BOOTSTRAP_ON_EMPTY="${CSIF_BOOTSTRAP_BASE_ON_EMPTY:-1}"
BOOTSTRAP_MODE="${CSIF_BOOTSTRAP_BASE_MODE:-ensure}"

if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
  docker rm -f "${CONTAINER_NAME}" >/dev/null
fi

docker run -d \
  --name "${CONTAINER_NAME}" \
  --restart unless-stopped \
  -p "${HOST_PORT}:8080" \
  -e CSIF_BANK_PATH=/data/my_brain.rwif \
  -e CSIF_BOOTSTRAP_BASE_ON_EMPTY="${BOOTSTRAP_ON_EMPTY}" \
  -e CSIF_BOOTSTRAP_BASE_MODE="${BOOTSTRAP_MODE}" \
  -v "${DATA_VOLUME}:/data" \
  "${IMAGE}"

echo "CSIF-Agent is running in container ${CONTAINER_NAME}."
echo "Test it with:"
echo "  curl -X POST http://localhost:${HOST_PORT}/teach -H 'Content-Type: application/json' -d '{\"text\":\"A whale is a mammal.\"}'"
echo "  curl -X POST http://localhost:${HOST_PORT}/query -H 'Content-Type: application/json' -d '{\"text\":\"What is a whale?\"}'"
