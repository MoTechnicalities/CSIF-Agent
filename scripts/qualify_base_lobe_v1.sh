#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${BASE_LOBE_PORT:-18081}"
SEED_METHOD="${SEED_METHOD:-localbulk}"
CSIF_SAVE_EVERY="${CSIF_SAVE_EVERY:-128}"
SEED_VERBOSE="${SEED_VERBOSE:-0}"
SEED_MAX_TIME="${SEED_MAX_TIME:-20}"
SEED_RETRIES="${SEED_RETRIES:-2}"
SEED_FACT_DELAY="${SEED_FACT_DELAY:-0.02}"
SEED_TIMEOUT_IS_SOFT="${SEED_TIMEOUT_IS_SOFT:-0}"
SEED_REQUIRE_ZERO_TIMEOUTS="${SEED_REQUIRE_ZERO_TIMEOUTS:-0}"
QUALIFY_SEED_MODE="${QUALIFY_SEED_MODE:-adaptive}"
TMP_DIR="$(mktemp -d -t csif-base-lobe-v1-XXXXXX)"
BANK_PATH="$TMP_DIR/base_lobe_v1_bank.rwif"
SERVER_LOG="$TMP_DIR/server.log"
SUMMARY_JSON="$TMP_DIR/benchmark_summary.json"
SEED_DIR="$ROOT_DIR/data/base_lobe_v1/seed"

now_ms() {
  date +%s%3N
}

TOTAL_START_MS="$(now_ms)"

# Adaptive mode avoids multi-hour retry loops when the server stays healthy but requests time out.
if [[ "$QUALIFY_SEED_MODE" == "adaptive" ]]; then
  SEED_MAX_TIME="8"
  SEED_RETRIES="1"
  SEED_FACT_DELAY="0.01"
  SEED_TIMEOUT_IS_SOFT="1"
  SEED_REQUIRE_ZERO_TIMEOUTS="0"
fi

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}

trap cleanup EXIT

cd "$ROOT_DIR"

# Clear stale runs from prior interrupted qualifications.
pkill -f "scripts/seed_base_lobe_v1.sh" >/dev/null 2>&1 || true
pkill -f "target/release/agent_demo" >/dev/null 2>&1 || true

python3 scripts/build_base_lobe_v1_assets.py
cargo build --release -p agent_demo >/dev/null
cargo build --release -p bulk_seed >/dev/null
if [[ "$SEED_METHOD" == "localbulk" ]]; then
  echo "Bulk seeding base lobe v1 into isolated bank (in-process)..."
  SEED_START_MS="$(now_ms)"
  set +e
  BULK_OUTPUT="$(CSIF_SAVE_EVERY="$CSIF_SAVE_EVERY" \
  "$ROOT_DIR/target/release/bulk_seed" "$BANK_PATH" "$ROOT_DIR/grammar.toml" "$SEED_DIR" 2>&1)"
  SEED_EXIT=$?
  set -e
  echo "$BULK_OUTPUT"
  SEED_END_MS="$(now_ms)"
  SEED_ELAPSED_MS=$((SEED_END_MS - SEED_START_MS))
  BULK_TAUGHT="$(echo "$BULK_OUTPUT" | sed -n 's/.*taught=\([0-9]\+\).*/\1/p' | tail -n1)"
  if [[ -n "$BULK_TAUGHT" && "$SEED_ELAPSED_MS" -gt 0 ]]; then
    BULK_TPS="$(python3 - <<'PY' "$BULK_TAUGHT" "$SEED_ELAPSED_MS"
import sys
taught = int(sys.argv[1])
elapsed_ms = int(sys.argv[2])
print(f"{(taught * 1000.0) / elapsed_ms:.1f}")
PY
)"
    echo "Seed timing: ${SEED_ELAPSED_MS}ms (${BULK_TPS} facts/s)"
  else
    echo "Seed timing: ${SEED_ELAPSED_MS}ms"
  fi
  if [[ "$SEED_EXIT" -ne 0 ]]; then
    echo
    echo "Bulk seeding failed with exit code $SEED_EXIT"
    exit "$SEED_EXIT"
  fi
else
  CSIF_BANK_PATH="$BANK_PATH" \
  CSIF_GRAMMAR_PATH="$ROOT_DIR/grammar.toml" \
  CSIF_PORT="$PORT" \
  CSIF_SAVE_EVERY="$CSIF_SAVE_EVERY" \
  "$ROOT_DIR/target/release/agent_demo" >"$SERVER_LOG" 2>&1 &
  SERVER_PID=$!

  for _ in {1..120}; do
    if curl -sS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
      break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
      echo "Server exited before becoming healthy."
      cat "$SERVER_LOG"
      exit 1
    fi
    sleep 0.25
  done

  if ! curl -sS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
    echo "Timed out waiting for health endpoint."
    cat "$SERVER_LOG"
    exit 1
  fi

  echo "Seeding base lobe v1 into isolated bank..."
  SEED_START_MS="$(now_ms)"
  set +e
  SEED_VERBOSE="${SEED_VERBOSE:-0}" \
  SEED_MAX_TIME="${SEED_MAX_TIME:-20}" \
  SEED_RETRIES="${SEED_RETRIES:-2}" \
  SEED_FACT_DELAY="${SEED_FACT_DELAY:-0.02}" \
  SEED_TIMEOUT_IS_SOFT="${SEED_TIMEOUT_IS_SOFT:-0}" \
  SEED_REQUIRE_ZERO_TIMEOUTS="${SEED_REQUIRE_ZERO_TIMEOUTS:-0}" \
  AGENT_URL="http://127.0.0.1:${PORT}" \
  bash scripts/seed_base_lobe_v1.sh
  SEED_EXIT=$?
  set -e
  SEED_END_MS="$(now_ms)"
  SEED_ELAPSED_MS=$((SEED_END_MS - SEED_START_MS))
  echo "Seed timing: ${SEED_ELAPSED_MS}ms"

  if [[ "$SEED_EXIT" -ne 0 ]]; then
    echo
    echo "Seeding failed with exit code $SEED_EXIT"
    echo "Server log tail:"
    tail -n 80 "$SERVER_LOG" || true
    exit "$SEED_EXIT"
  fi
fi

CSIF_BANK_PATH="$BANK_PATH" \
CSIF_GRAMMAR_PATH="$ROOT_DIR/grammar.toml" \
CSIF_PORT="$PORT" \
CSIF_SAVE_EVERY="$CSIF_SAVE_EVERY" \
"$ROOT_DIR/target/release/agent_demo" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

for _ in {1..120}; do
  if curl -sS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "Server exited before becoming healthy."
    cat "$SERVER_LOG"
    exit 1
  fi
  sleep 0.25
done

if ! curl -sS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
  echo "Timed out waiting for health endpoint."
  cat "$SERVER_LOG"
  exit 1
fi

echo "Running benchmark..."
BENCH_START_MS="$(now_ms)"
set +e
BENCHMARK_VERBOSE=0 \
BENCHMARK_HTTP_TIMEOUT=8 \
BENCHMARK_HTTP_RETRIES=1 \
python3 scripts/run_base_lobe_v1_benchmark.py "http://127.0.0.1:${PORT}" "$SUMMARY_JSON"
BENCH_EXIT=$?
set -e
BENCH_END_MS="$(now_ms)"
BENCH_ELAPSED_MS=$((BENCH_END_MS - BENCH_START_MS))
echo "Benchmark timing: ${BENCH_ELAPSED_MS}ms"

echo
if [[ -f "$SUMMARY_JSON" ]]; then
  echo "Benchmark summary JSON: $SUMMARY_JSON"
  cat "$SUMMARY_JSON"
  if [[ "$BENCH_ELAPSED_MS" -gt 0 ]]; then
    BENCH_TOTAL="$(python3 - <<'PY' "$SUMMARY_JSON"
import json, sys
with open(sys.argv[1], 'r', encoding='utf-8') as f:
    payload = json.load(f)
print(payload.get('total', 0))
PY
)"
    if [[ "$BENCH_TOTAL" -gt 0 ]]; then
      BENCH_QPS="$(python3 - <<'PY' "$BENCH_TOTAL" "$BENCH_ELAPSED_MS"
import sys
total = int(sys.argv[1])
elapsed_ms = int(sys.argv[2])
print(f"{(total * 1000.0) / elapsed_ms:.1f}")
PY
)"
      echo "Benchmark throughput: ${BENCH_QPS} queries/s"
    fi
  fi
fi

echo
echo "Qualification workspace: $TMP_DIR"
TOTAL_END_MS="$(now_ms)"
TOTAL_ELAPSED_MS=$((TOTAL_END_MS - TOTAL_START_MS))
echo "Qualification timing: ${TOTAL_ELAPSED_MS}ms"

if [[ "$BENCH_EXIT" -ne 0 ]]; then
  echo "Qualification result: FAIL (see benchmark summary above)."
  exit "$BENCH_EXIT"
fi

echo "Qualification result: PASS"
