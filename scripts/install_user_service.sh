#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
ENV_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/csif-agent"
UNIT_PATH="$UNIT_DIR/csif-agent.service"
ENV_PATH="$ENV_DIR/csif-agent.env"

mkdir -p "$UNIT_DIR" "$ENV_DIR"
mkdir -p "$ROOT_DIR/.runtime"

if [[ ! -x "$ROOT_DIR/target/release/agent_demo" ]]; then
  echo "Release binary missing; building..."
  (cd "$ROOT_DIR" && cargo build -p agent_demo --release)
fi

if [[ ! -f "$ENV_PATH" ]]; then
  cat > "$ENV_PATH" <<EOF
CSIF_BANK_PATH=$ROOT_DIR/.runtime/deploy_bank.rwif
CSIF_GRAMMAR_PATH=$ROOT_DIR/grammar.toml
CSIF_PORT=19191
CSIF_COMPUTE_LATEX=1
CSIF_EXEC_APPROVAL_TOKEN=change-me-approval-token
CSIF_ADMIN_TOKEN=change-me-admin-token
CSIF_EXEC_AUDIT_LOG_PATH=$ROOT_DIR/.runtime/execute_audit.jsonl
EOF
  echo "Created env file: $ENV_PATH"
  echo "Update tokens in that file before exposing the service outside localhost."
fi

cat > "$UNIT_PATH" <<EOF
[Unit]
Description=CSIF Agent Service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=$ROOT_DIR
EnvironmentFile=$ENV_PATH
ExecStart=$ROOT_DIR/target/release/agent_demo
Restart=always
RestartSec=3
NoNewPrivileges=true
LimitNOFILE=8192

[Install]
WantedBy=default.target
EOF

echo "Installed unit: $UNIT_PATH"

if command -v systemctl >/dev/null 2>&1; then
  systemctl --user daemon-reload
  systemctl --user enable --now csif-agent.service
  echo "Service started."
  systemctl --user --no-pager --full status csif-agent.service | head -40 || true
  echo
  echo "Probe command:"
  echo "CSIF_PROBE_BASE_URL=http://127.0.0.1:\${CSIF_PORT:-19191} CSIF_EXEC_APPROVAL_TOKEN=... CSIF_ADMIN_TOKEN=... $ROOT_DIR/scripts/probe_production.py"
else
  echo "systemctl not available. Unit file created, but not started."
fi
