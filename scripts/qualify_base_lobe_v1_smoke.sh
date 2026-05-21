#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${BASE_LOBE_PORT:-18181}"
SMOKE_LINES_PER_SEED="${SMOKE_LINES_PER_SEED:-25}"
SMOKE_BENCHMARK_LINES="${SMOKE_BENCHMARK_LINES:-60}"
SMOKE_BENCHMARK_PROFILE="${SMOKE_BENCHMARK_PROFILE:-balanced_3x50}"
SMOKE_SKIP_SEED="${SMOKE_SKIP_SEED:-1}"
SEED_VERBOSE="${SEED_VERBOSE:-0}"
SEED_MAX_TIME="${SEED_MAX_TIME:-2}"
SEED_RETRIES="${SEED_RETRIES:-1}"
SEED_FACT_DELAY="${SEED_FACT_DELAY:-0.005}"
SEED_TIMEOUT_IS_SOFT="${SEED_TIMEOUT_IS_SOFT:-1}"
SEED_REQUIRE_ZERO_TIMEOUTS="${SEED_REQUIRE_ZERO_TIMEOUTS:-0}"
BENCHMARK_HTTP_TIMEOUT="${BENCHMARK_HTTP_TIMEOUT:-6}"
BENCHMARK_HTTP_RETRIES="${BENCHMARK_HTTP_RETRIES:-1}"
CSIF_SAVE_EVERY="${CSIF_SAVE_EVERY:-128}"

TMP_DIR="$(mktemp -d -t csif-base-lobe-v1-smoke-XXXXXX)"
BANK_PATH="$TMP_DIR/base_lobe_v1_bank.rwif"
SERVER_LOG="$TMP_DIR/server.log"
SUMMARY_JSON="$TMP_DIR/benchmark_summary.json"
SMOKE_SEED_DIR="$TMP_DIR/seed"
SMOKE_BENCHMARK_PATH="$TMP_DIR/benchmark_smoke.jsonl"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}

trap cleanup EXIT

mkdir -p "$SMOKE_SEED_DIR"
cd "$ROOT_DIR"

python3 scripts/build_base_lobe_v1_assets.py
cargo build --release -p agent_demo >/dev/null

# Build sampled seed files using the same filenames expected by seed_base_lobe_v1.sh.
for category in taxonomy causality properties geography operator_utility; do
  src="data/base_lobe_v1/seed/${category}.txt"
  dst="$SMOKE_SEED_DIR/${category}.txt"
  head -n "$SMOKE_LINES_PER_SEED" "$src" > "$dst"
done

ROOT_DIR="$ROOT_DIR" \
SMOKE_SEED_DIR="$SMOKE_SEED_DIR" \
SMOKE_BENCHMARK_PATH="$SMOKE_BENCHMARK_PATH" \
SMOKE_BENCHMARK_LINES="$SMOKE_BENCHMARK_LINES" \
SMOKE_BENCHMARK_PROFILE="$SMOKE_BENCHMARK_PROFILE" \
python3 - <<'PY'
import json
import os
from pathlib import Path

root = Path(os.environ['ROOT_DIR'])
seed_dir = Path(os.environ['SMOKE_SEED_DIR'])
out_path = Path(os.environ['SMOKE_BENCHMARK_PATH'])
target = int(os.environ['SMOKE_BENCHMARK_LINES'])
profile = os.environ.get('SMOKE_BENCHMARK_PROFILE', 'balanced_3x50')

taxonomy = [line.strip() for line in (seed_dir / 'taxonomy.txt').read_text(encoding='utf-8').splitlines() if line.strip()]
causality = [line.strip() for line in (seed_dir / 'causality.txt').read_text(encoding='utf-8').splitlines() if line.strip()]
properties = [line.strip() for line in (seed_dir / 'properties.txt').read_text(encoding='utf-8').splitlines() if line.strip()]
geography = [line.strip() for line in (seed_dir / 'geography.txt').read_text(encoding='utf-8').splitlines() if line.strip()]
operator_utility = [line.strip() for line in (seed_dir / 'operator_utility.txt').read_text(encoding='utf-8').splitlines() if line.strip()]

rows = []

def add_taxonomy(limit: int):
  seen_subjects = set()
  idx = 0
  for fact in taxonomy:
    subject, sep, obj = fact.partition(' is a ')
    if not sep:
      subject, sep, obj = fact.partition(' is an ')
    if not obj:
      continue
    subject_norm = subject.removeprefix('a ').removeprefix('an ').strip()
    if subject_norm in seen_subjects:
      continue
    seen_subjects.add(subject_norm)
    idx += 1
    rows.append({
      'id': f'smoke-taxo-{idx:04d}',
      'type': 'teach',
      'teach': fact,
      'expected_mode': 'contains',
      'expected': '[TEACHING]',
      'category': 'taxonomy',
    })
    if idx >= limit:
      break

def add_teach(category: str, facts: list[str], limit: int, start_idx: int = 1):
  for idx, fact in enumerate(facts[:limit], start=start_idx):
    rows.append({
      'id': f'smoke-{category}-{idx:04d}',
      'type': 'teach',
      'teach': fact,
      'expected_mode': 'contains',
      'expected': '[TEACHING]',
      'category': category,
    })

if profile == 'balanced_3x50':
  add_taxonomy(50)
  add_teach('causality', causality, 50)
  add_teach('properties', properties, 50)
else:
  add_taxonomy(min(50, len(taxonomy)))
  add_teach('causality', causality, min(50, len(causality)))
  add_teach('properties', properties, min(50, len(properties)))

if len(rows) < target:
  add_teach('geography', geography, min(target - len(rows), len(geography)))
if len(rows) < target:
  add_teach('operator_utility', operator_utility, min(target - len(rows), len(operator_utility)))

rows = rows[:target]
with out_path.open('w', encoding='utf-8') as f:
  for row in rows:
    f.write(json.dumps(row, ensure_ascii=True) + '\n')

print(f'Generated smoke benchmark rows={len(rows)} profile={profile} path={out_path}')
PY

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
  sleep 0.1
done

if ! curl -sS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
  echo "Timed out waiting for health endpoint."
  cat "$SERVER_LOG"
  exit 1
fi

if [[ "$SMOKE_SKIP_SEED" == "1" ]]; then
  echo "Skipping pre-seed for smoke gate (SMOKE_SKIP_SEED=1)."
else
  echo "Smoke seeding into isolated bank..."
  set +e
  SEED_DIR="$SMOKE_SEED_DIR" \
  SEED_LOCK_FILE="$TMP_DIR/seed.lock" \
  SEED_VERBOSE="$SEED_VERBOSE" \
  SEED_MAX_TIME="$SEED_MAX_TIME" \
  SEED_RETRIES="$SEED_RETRIES" \
  SEED_FACT_DELAY="$SEED_FACT_DELAY" \
  SEED_TIMEOUT_IS_SOFT="$SEED_TIMEOUT_IS_SOFT" \
  SEED_REQUIRE_ZERO_TIMEOUTS="$SEED_REQUIRE_ZERO_TIMEOUTS" \
  AGENT_URL="http://127.0.0.1:${PORT}" \
  bash scripts/seed_base_lobe_v1.sh
  SEED_EXIT=$?
  set -e

  if [[ "$SEED_EXIT" -ne 0 ]]; then
    echo
    echo "Smoke seeding failed with exit code $SEED_EXIT"
    echo "Server log tail:"
    tail -n 80 "$SERVER_LOG" || true
    echo
    echo "Smoke workspace: $TMP_DIR"
    exit "$SEED_EXIT"
  fi
fi

echo "Running smoke benchmark..."
set +e
BENCHMARK_VERBOSE=0 \
BENCHMARK_HTTP_TIMEOUT="$BENCHMARK_HTTP_TIMEOUT" \
BENCHMARK_HTTP_RETRIES="$BENCHMARK_HTTP_RETRIES" \
BENCHMARK_PATH="$SMOKE_BENCHMARK_PATH" \
python3 scripts/run_base_lobe_v1_benchmark.py "http://127.0.0.1:${PORT}" "$SUMMARY_JSON"
BENCH_EXIT=$?
set -e

echo
if [[ -f "$SUMMARY_JSON" ]]; then
  echo "Smoke benchmark summary JSON: $SUMMARY_JSON"
  cat "$SUMMARY_JSON"
fi

echo
ls -lh "$BANK_PATH" || true
echo "Smoke workspace: $TMP_DIR"

if [[ "$BENCH_EXIT" -ne 0 ]]; then
  echo "Smoke qualification result: FAIL"
  exit "$BENCH_EXIT"
fi

echo "Smoke qualification result: PASS"
