#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
MODE="${1:-all}"
DOCKER_IMAGE="${DOCKER_IMAGE:-csif-agent:freeze-local}"
DOCKER_CONTAINER_NAME="${DOCKER_CONTAINER_NAME:-csif-agent-freeze-gate}"
DOCKER_VOLUME="${DOCKER_VOLUME:-csif-agent-freeze-gate-data}"
DOCKER_HOST_PORT="${DOCKER_HOST_PORT:-28080}"
KEEP_DOCKER_ARTIFACTS="${KEEP_DOCKER_ARTIFACTS:-0}"
FREEZE_SUMMARY_PATH="${FREEZE_SUMMARY_PATH:-$ROOT_DIR/.runtime/native/freeze_gate_latest.txt}"

mkdir -p "$(dirname "$FREEZE_SUMMARY_PATH")"
: > "$FREEZE_SUMMARY_PATH"

log() {
  echo "$*"
  echo "$*" >> "$FREEZE_SUMMARY_PATH"
}

wait_health() {
  local api="$1"
  local attempts="${2:-120}"
  local delay_secs="${3:-0.25}"
  local i
  for i in $(seq 1 "$attempts"); do
    if curl -sS "$api/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep "$delay_secs"
  done
  return 1
}

run_profile_benchmark() {
  local api="$1"
  local profile="$2"
  local bench_path
  local summary_path="$ROOT_DIR/.runtime/native/freeze_${profile}_summary.json"
  local log_path="$ROOT_DIR/.runtime/native/freeze_${profile}.log"

  case "$profile" in
    v2)
      bench_path="$ROOT_DIR/data/intellectualization_pack_v2/benchmarks/intellectualization_pack_v2_benchmark.jsonl"
      ;;
    anti-v3)
      bench_path="$ROOT_DIR/data/intellectualization_pack_v3/benchmarks/anti_lesson_v3_benchmark.jsonl"
      ;;
    *)
      log "unsupported profile: $profile"
      return 2
      ;;
  esac

  set +e
  BENCHMARK_PATH="$bench_path" \
  BENCHMARK_HTTP_TIMEOUT=6 \
  BENCHMARK_HTTP_RETRIES=2 \
  python3 "$ROOT_DIR/scripts/run_base_lobe_v1_benchmark.py" "$api" "$summary_path" > "$log_path" 2>&1
  local ec=$?
  set -e

  if [[ -f "$summary_path" ]]; then
    local stats
    stats="$(python3 - <<'PY' "$summary_path"
import json,sys
p=json.load(open(sys.argv[1], 'r', encoding='utf-8'))
print(f"{p.get('passed')}/{p.get('total')} failed={p.get('failed')}")
PY
)"
    log "${profile}_summary=${stats}"
  else
    log "${profile}_summary=missing"
  fi

  if [[ "$ec" -ne 0 ]]; then
    log "${profile}_benchmark=FAIL (exit=$ec)"
    log "${profile}_log_tail:"
    tail -n 20 "$log_path" >> "$FREEZE_SUMMARY_PATH" || true
  else
    log "${profile}_benchmark=PASS"
  fi

  return "$ec"
}

run_native_gate() {
  local api="http://127.0.0.1:18080"
  local native_started=0
  local v2_ec=0
  local anti_ec=0
  local math_ec=0

  log "[native] gate_start"

  if ! wait_health "$api" 1 0.1; then
    log "[native] starting runtime on 18080"
    nohup "$ROOT_DIR/scripts/native/start.sh" > /tmp/csif_native_freeze_gate.log 2>&1 &
    native_started=1
  fi

  if ! wait_health "$api" 120 0.25; then
    log "[native] health=FAIL"
    log "[native] runtime_log_tail:"
    tail -n 40 /tmp/csif_native_freeze_gate.log >> "$FREEZE_SUMMARY_PATH" || true
    return 1
  fi

  log "[native] health=PASS"
  run_profile_benchmark "$api" v2 || v2_ec=$?
  run_profile_benchmark "$api" anti-v3 || anti_ec=$?

  set +e
  bash "$ROOT_DIR/scripts/qualify_math_attacks_smoke.sh" > "$ROOT_DIR/.runtime/native/freeze_math_attacks.log" 2>&1
  math_ec=$?
  set -e

  if [[ "$math_ec" -eq 0 ]]; then
    log "math_attacks=PASS"
  else
    log "math_attacks=FAIL (exit=$math_ec)"
    log "math_attacks_log_tail:"
    tail -n 30 "$ROOT_DIR/.runtime/native/freeze_math_attacks.log" >> "$FREEZE_SUMMARY_PATH" || true
  fi

  log "[native] started_runtime=${native_started}"
  if [[ "$v2_ec" -eq 0 && "$anti_ec" -eq 0 && "$math_ec" -eq 0 ]]; then
    log "[native] verdict=PASS"
    return 0
  fi

  log "[native] verdict=FAIL"
  return 1
}

run_docker_math_smoke() {
  local api="$1"
  python3 - <<'PY' "$api"
import json
import sys
import urllib.request

api = sys.argv[1].rstrip("/")
checks = [
    ("What is 2 + 2?", "= 4"),
    ("What is 19 * 7?", "= 133"),
    ("Is a whale a mammal?", "YES: a whale is a mammal."),
    ("Is a whale an animal?", "YES: a whale is an animal."),
]

for q, expected in checks:
    req = urllib.request.Request(
        f"{api}/query",
        data=json.dumps({"text": q}).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=8) as resp:
        body = json.loads(resp.read().decode("utf-8"))
    answer = body.get("answer", "")
    if expected not in answer:
        print(f"FAIL query={q} expected~={expected} got={answer}")
        sys.exit(1)
    print(f"PASS query={q} -> {answer}")
PY
}

cleanup_docker_gate() {
  if [[ "$KEEP_DOCKER_ARTIFACTS" == "1" ]]; then
    return 0
  fi
  docker rm -f "$DOCKER_CONTAINER_NAME" >/dev/null 2>&1 || true
  docker volume rm "$DOCKER_VOLUME" >/dev/null 2>&1 || true
}

run_docker_gate() {
  local api="http://127.0.0.1:${DOCKER_HOST_PORT}"
  local v2_ec=0
  local anti_ec=0
  local math_ec=0

  log "[docker] gate_start image=${DOCKER_IMAGE} host_port=${DOCKER_HOST_PORT}"

  docker rm -f "$DOCKER_CONTAINER_NAME" >/dev/null 2>&1 || true
  if [[ "$KEEP_DOCKER_ARTIFACTS" != "1" ]]; then
    docker volume rm "$DOCKER_VOLUME" >/dev/null 2>&1 || true
  fi

  docker build -t "$DOCKER_IMAGE" "$ROOT_DIR" >/tmp/csif_docker_freeze_build.log 2>&1

  docker run -d \
    --name "$DOCKER_CONTAINER_NAME" \
    -p "${DOCKER_HOST_PORT}:8080" \
    -e CSIF_BANK_PATH=/data/my_brain.rwif \
    -e CSIF_BOOTSTRAP_BASE_ON_EMPTY=0 \
    -e CSIF_BOOTSTRAP_BASE_MODE=empty \
    -e CSIF_LOBES_DIR=/app/lobes \
    -e CSIF_PLAY_ENABLED=0 \
    -e CSIF_OBSERVE_ENABLED=0 \
    -v "$DOCKER_VOLUME:/data" \
    "$DOCKER_IMAGE" >/tmp/csif_docker_freeze_run.log

  # First container boot may take longer due base bootstrap seeding.
  if ! wait_health "$api" 600 0.5; then
    log "[docker] health=FAIL"
    log "[docker] container_logs_tail:"
    docker logs "$DOCKER_CONTAINER_NAME" --tail 60 >> "$FREEZE_SUMMARY_PATH" || true
    log "[docker] container_state:"
    docker ps -a --filter "name=$DOCKER_CONTAINER_NAME" --format '{{.Names}} {{.Status}}' >> "$FREEZE_SUMMARY_PATH" || true
    cleanup_docker_gate
    return 1
  fi

  log "[docker] health=PASS"

  run_profile_benchmark "$api" v2 || v2_ec=$?
  run_profile_benchmark "$api" anti-v3 || anti_ec=$?

  set +e
  run_docker_math_smoke "$api" > "$ROOT_DIR/.runtime/native/freeze_docker_math_smoke.log" 2>&1
  math_ec=$?
  set -e

  if [[ "$math_ec" -eq 0 ]]; then
    log "[docker] math_smoke=PASS"
  else
    log "[docker] math_smoke=FAIL (exit=$math_ec)"
    log "[docker] math_smoke_tail:"
    tail -n 20 "$ROOT_DIR/.runtime/native/freeze_docker_math_smoke.log" >> "$FREEZE_SUMMARY_PATH" || true
  fi

  log "[docker] lobe_load_tail:"
  docker logs "$DOCKER_CONTAINER_NAME" --tail 20 >> "$FREEZE_SUMMARY_PATH" || true

  local verdict=0
  if [[ "$v2_ec" -ne 0 || "$anti_ec" -ne 0 || "$math_ec" -ne 0 ]]; then
    verdict=1
  fi

  if [[ "$verdict" -eq 0 ]]; then
    log "[docker] verdict=PASS"
  else
    log "[docker] verdict=FAIL"
  fi

  cleanup_docker_gate
  return "$verdict"
}

main() {
  local native_ec=0
  local docker_ec=0

  case "$MODE" in
    native)
      run_native_gate || native_ec=$?
      ;;
    docker)
      run_docker_gate || docker_ec=$?
      ;;
    all)
      run_native_gate || native_ec=$?
      run_docker_gate || docker_ec=$?
      ;;
    *)
      echo "usage: $0 [native|docker|all]"
      exit 2
      ;;
  esac

  log "native_exit=${native_ec}"
  log "docker_exit=${docker_ec}"

  if [[ "$native_ec" -eq 0 && "$docker_ec" -eq 0 ]]; then
    log "FREEZE_GATE_VERDICT=PASS"
    exit 0
  fi

  if [[ "$MODE" == "native" && "$native_ec" -eq 0 ]]; then
    log "FREEZE_GATE_VERDICT=PASS"
    exit 0
  fi

  if [[ "$MODE" == "docker" && "$docker_ec" -eq 0 ]]; then
    log "FREEZE_GATE_VERDICT=PASS"
    exit 0
  fi

  log "FREEZE_GATE_VERDICT=FAIL"
  exit 1
}

main "$@"
