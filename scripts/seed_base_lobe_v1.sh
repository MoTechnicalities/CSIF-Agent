#!/usr/bin/env bash

set -euo pipefail

LOCK_FILE="${SEED_LOCK_FILE:-/tmp/csif_base_lobe_v1_seed.lock}"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
  echo "Another seed_base_lobe_v1.sh process is already running."
  exit 3
fi

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SEED_DIR="${SEED_DIR:-$ROOT_DIR/data/base_lobe_v1/seed}"
AGENT_URL="${AGENT_URL:-http://localhost:18080}"
FAILED=0
TAUGHT=0
SOFT_TIMEOUTS=0
SEED_VERBOSE="${SEED_VERBOSE:-1}"
SEED_MAX_TIME="${SEED_MAX_TIME:-20}"
SEED_RETRIES="${SEED_RETRIES:-5}"
SEED_FACT_DELAY="${SEED_FACT_DELAY:-0.02}"
SEED_TIMEOUT_IS_SOFT="${SEED_TIMEOUT_IS_SOFT:-0}"
SEED_REQUIRE_ZERO_TIMEOUTS="${SEED_REQUIRE_ZERO_TIMEOUTS:-0}"

if [[ ! -d "$SEED_DIR" ]]; then
  echo "Seed directory missing: $SEED_DIR"
  exit 1
fi

teach_file() {
  local file="$1"
  echo "Seeding: $(basename "$file")"

  while IFS= read -r fact; do
    [[ -z "$fact" ]] && continue
    payload=$(printf '{"text":"%s"}' "$fact")
    success=0
    last_err=""
    last_code=0
    for ((attempt=1; attempt<=SEED_RETRIES; attempt++)); do
      if response=$(curl -sS --max-time "$SEED_MAX_TIME" -X POST "$AGENT_URL/teach" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/tmp/csif_seed_err.log); then
        success=1
        break
      fi
      last_code=$?
      last_err="$(cat /tmp/csif_seed_err.log 2>/dev/null || true)"
      sleep 0.15
    done

    if [[ "$success" -eq 1 ]]; then
      TAUGHT=$((TAUGHT + 1))
      if [[ "$SEED_VERBOSE" == "1" ]]; then
        echo "  $fact"
        echo "  -> $response"
      fi
    else
      err_msg="$last_err"
      FAILED=$((FAILED + 1))
      health_status="down"
      if curl -sS --max-time 1 "$AGENT_URL/health" >/dev/null 2>&1; then
        health_status="up"
      fi

      # In adaptive mode, treat request timeouts as soft failures when the server is healthy.
      if [[ "$SEED_TIMEOUT_IS_SOFT" == "1" && "$health_status" == "up" && "$last_code" -eq 28 ]]; then
        FAILED=$((FAILED - 1))
        SOFT_TIMEOUTS=$((SOFT_TIMEOUTS + 1))
        echo "  $fact"
        echo "  -> SOFT TIMEOUT: $err_msg"
        echo "  -> health during failure: $health_status"
        echo "  -> continuing (adaptive mode)"
      else
        echo "  $fact"
        echo "  -> ERROR: $err_msg"
        echo "  -> health during failure: $health_status"
      fi
    fi

    sleep "$SEED_FACT_DELAY"
  done < "$file"
}

for category_file in \
  "$SEED_DIR/taxonomy.txt" \
  "$SEED_DIR/causality.txt" \
  "$SEED_DIR/properties.txt" \
  "$SEED_DIR/geography.txt" \
  "$SEED_DIR/operator_utility.txt"
  do
  teach_file "$category_file"
done

echo "Skipping arithmetic seed ingest for now (compute is query-native, not teach-native in v1.4)."

echo "Seed summary: taught=$TAUGHT soft_timeouts=$SOFT_TIMEOUTS failed=$FAILED"
if [[ "$SEED_REQUIRE_ZERO_TIMEOUTS" == "1" && "$SOFT_TIMEOUTS" -gt 0 ]]; then
  echo "Seed pass completed with soft timeouts (configured as fatal)."
  exit 2
fi
if [[ "$FAILED" -gt 0 ]]; then
  echo "Seed pass completed with failures."
  exit 2
fi

echo "Base lobe v1 seed pass complete."
