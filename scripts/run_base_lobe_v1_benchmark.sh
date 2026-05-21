#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
AGENT_URL="${AGENT_URL:-http://localhost:18080}"

cd "$ROOT_DIR"
python3 scripts/run_base_lobe_v1_benchmark.py "$AGENT_URL"
