#!/bin/sh
set -eu

BANK_PATH="${CSIF_BANK_PATH:-/data/my_brain.rwif}"
GRAMMAR_PATH="${CSIF_GRAMMAR_PATH:-/app/grammar.toml}"
BASE_SEED_DIR="${CSIF_BASE_SEED_DIR:-/app/data/base_lobe_v1/seed}"
BOOTSTRAP_ON_EMPTY="${CSIF_BOOTSTRAP_BASE_ON_EMPTY:-1}"

is_enabled() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

bank_is_empty() {
  if [ ! -f "$BANK_PATH" ] || [ ! -s "$BANK_PATH" ]; then
    return 0
  fi

  if grep -Eq '"edges"[[:space:]]*:[[:space:]]*\{[[:space:]]*\}' "$BANK_PATH"; then
    return 0
  fi

  return 1
}

if is_enabled "$BOOTSTRAP_ON_EMPTY"; then
  if bank_is_empty; then
    echo "[bootstrap] Empty bank detected at $BANK_PATH"
    echo "[bootstrap] Seeding base lobe from $BASE_SEED_DIR"

    mkdir -p "$(dirname "$BANK_PATH")"
    /app/bulk_seed "$BANK_PATH" "$GRAMMAR_PATH" "$BASE_SEED_DIR"

    echo "[bootstrap] Base lobe seed complete"
  else
    echo "[bootstrap] Existing bank detected; skipping base seed"
  fi
else
  echo "[bootstrap] Disabled by CSIF_BOOTSTRAP_BASE_ON_EMPTY=$BOOTSTRAP_ON_EMPTY"
fi

exec /app/agent_demo
