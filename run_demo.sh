#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
PORT="${CSIF_PORT:-18080}"
TMP_DIR="$(mktemp -d -t csif-agent-demo-XXXXXX)"
BANK_PATH="$TMP_DIR/demo_bank.rwif"
LOG_PATH="$TMP_DIR/server.log"

cleanup() {
	if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
		kill "$SERVER_PID" 2>/dev/null || true
		wait "$SERVER_PID" 2>/dev/null || true
	fi
	rm -rf "$TMP_DIR"
}

trap cleanup EXIT

post_json() {
	local path="$1"
	local payload="$2"
	echo
	echo "> POST ${path}"
	echo "> Payload: ${payload}"
	curl -sS -X POST "http://127.0.0.1:${PORT}${path}" \
		-H "Content-Type: application/json" \
		-d "$payload"
	echo
}

echo "== CSIF-Agent local demo =="
echo "Root: $ROOT_DIR"
echo "Port: $PORT"
echo "Temporary bank: $BANK_PATH"

cd "$ROOT_DIR"
cargo build --release -p agent_demo >/dev/null

CSIF_BANK_PATH="$BANK_PATH" \
CSIF_GRAMMAR_PATH="$ROOT_DIR/grammar.toml" \
CSIF_PORT="$PORT" \
"$ROOT_DIR/target/release/agent_demo" >"$LOG_PATH" 2>&1 &
SERVER_PID=$!

for _ in {1..60}; do
	if curl -sS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
		break
	fi
	if ! kill -0 "$SERVER_PID" 2>/dev/null; then
		echo "Server exited before becoming healthy."
		cat "$LOG_PATH"
		exit 1
	fi
	sleep 0.25
done

if ! curl -sS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
	echo "Timed out waiting for server health check."
	cat "$LOG_PATH"
	exit 1
fi

echo
echo "> GET /health"
curl -sS "http://127.0.0.1:${PORT}/health"
echo

post_json "/teach" '{"text":"a whale is a mammal"}'
post_json "/teach" '{"text":"a mammal is an animal"}'
post_json "/query" '{"text":"What is a whale?"}'
post_json "/query" '{"text":"Is a whale an animal?"}'
post_json "/teach" '{"text":"rain causes wet ground"}'
post_json "/teach" '{"text":"wet ground causes slippery"}'
post_json "/query" '{"text":"Does rain cause slippery?"}'
post_json "/teach" '{"text":"a whale has warm-blooded"}'
post_json "/query" '{"text":"Does a whale have warm-blooded?"}'
post_json "/query" '{"text":"What is 2 + 2?"}'
post_json "/teach" '{"text":"a whale is a fish"}'

echo
echo "Demo complete. Server log was captured in: $LOG_PATH"