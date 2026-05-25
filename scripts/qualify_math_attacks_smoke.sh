#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${MATH_ATTACKS_PORT:-18182}"
TMP_DIR="$(mktemp -d -t csif-math-attacks-XXXXXX)"
BANK_PATH="${MATH_ATTACKS_BANK_PATH:-$TMP_DIR/math_attacks_bank.rwif}"
AUDIT_LOG_PATH="${CSIF_EXEC_AUDIT_LOG_PATH:-$TMP_DIR/execute_audit.jsonl}"
SERVER_LOG="$TMP_DIR/server.log"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -f "$SERVER_LOG"
  if [[ -z "${MATH_ATTACKS_BANK_PATH:-}" ]]; then
    rm -f "$BANK_PATH"
  fi
  rm -rf "$TMP_DIR"
}

trap cleanup EXIT

start_server() {
  CSIF_BANK_PATH="$BANK_PATH" \
  CSIF_GRAMMAR_PATH="$ROOT_DIR/grammar.toml" \
  CSIF_PORT="$PORT" \
  CSIF_EXEC_APPROVAL_TOKEN="${CSIF_EXEC_APPROVAL_TOKEN:-smoke-approval-token}" \
  CSIF_ADMIN_TOKEN="${CSIF_ADMIN_TOKEN:-smoke-admin-token}" \
  CSIF_EXEC_AUDIT_LOG_PATH="$AUDIT_LOG_PATH" \
  CSIF_COMPUTE_LATEX=1 \
  "$ROOT_DIR/target/release/agent_demo" >"$SERVER_LOG" 2>&1 &
  SERVER_PID=$!
}

wait_for_server() {
  for _ in {1..120}; do
    if curl -sS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
      return 0
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
}

cd "$ROOT_DIR"
cargo build -p agent_demo --release >/dev/null

start_server
wait_for_server

MATH_ATTACKS_PORT="$PORT" CSIF_EXEC_APPROVAL_TOKEN="${CSIF_EXEC_APPROVAL_TOKEN:-smoke-approval-token}" CSIF_ADMIN_TOKEN="${CSIF_ADMIN_TOKEN:-smoke-admin-token}" CSIF_EXEC_AUDIT_LOG_PATH="$AUDIT_LOG_PATH" python3 - <<'PY'
import json
import os
import subprocess
import sys

base = f"http://127.0.0.1:{os.environ.get('MATH_ATTACKS_PORT', '18182')}"
cases = [
    ('solve x^2 - 5x + 6 >= 0', '[CRYSTAL] [SOLVE] x in (-inf, 2] U [3, +inf)'),
    ('solve (x-1)/(x+1) > 0', '[CRYSTAL] [SOLVE] x in (-inf, -1) U (1, +inf) (domain: x != -1)'),
  ('solve |(x+1)/(x-1)| <= 1', '[CRYSTAL] [SOLVE] x in (-inf, 0] (domain: x != 1)'),
  ('solve 0 <= x^2/(x-1) < 3', '[CRYSTAL] [SOLVE] x in {0} (domain: x != 1)'),
    ('solve |(x+1)/(x-1)| = 2', '[CRYSTAL] [SOLVE] x in {1/3, 3} (domain: x != 1)'),
    ('solve sqrt(x+3) = 2', '[CRYSTAL] [SOLVE] x = 1 (domain: x >= -3)'),
    ('solve sqrt(x-1) <= 2', '[CRYSTAL] [SOLVE] x in [1, 5] (domain: x >= 1)'),
    ('solve sqrt(x^2-4) = 0', '[CRYSTAL] [SOLVE] x in {-2, 2} (domain: x in (-inf, -2] U [2, +inf))'),
    ('solve x + y + z = 6; 2x - y + z = 3; -x + 2y + 3z = 12', '[CRYSTAL] [SOLVE] x1 = 1, x2 = 2, x3 = 3'),
    ('solve x + y + z = 6; 2x + 2y + 2z = 12; x + y + z = 7', '[CRYSTAL] [SOLVE] no solution'),
    ('solve x + y + z = 6; 2x + 2y + 2z = 12; x - y + z = 2', '[CRYSTAL] [SOLVE] infinitely many solutions'),
]

for query, expected_prefix in cases:
    payload = json.dumps({'text': query})
    out = subprocess.check_output([
        'curl', '-sS', '-X', 'POST', f'{base}/query',
        '-H', 'Content-Type: application/json', '-d', payload,
    ], text=True).strip()
    answer = json.loads(out)['answer']
    if not answer.startswith(expected_prefix):
        print(f'FAIL: {query}\nexpected prefix: {expected_prefix}\nactual: {answer}')
        sys.exit(1)
    print(f'PASS: {query} -> {answer.splitlines()[0]}')

proof_query = json.dumps({'text': 'solve (x+1)/(x-1) = 0'})
proof_out = subprocess.check_output([
    'curl', '-sS', '-X', 'POST', f'{base}/query',
    '-H', 'Content-Type: application/json', '-d', proof_query,
], text=True).strip()
proof_response = json.loads(proof_out)
certificate = proof_response.get('certificate')
if not certificate:
    print('FAIL: proof query did not return certificate')
    sys.exit(1)
if certificate.get('domain') != 'math':
  print(f"FAIL: expected math proof certificate, got: {certificate}")
  sys.exit(1)

verify_payload = json.dumps({'certificate': certificate})
verify_out = subprocess.check_output([
    'curl', '-sS', '-X', 'POST', f'{base}/verify-proof',
    '-H', 'Content-Type: application/json', '-d', verify_payload,
], text=True).strip()
verify_response = json.loads(verify_out)
if not verify_response.get('ok'):
    print(f"FAIL: verify-proof rejected certificate: {verify_response}")
    sys.exit(1)
print(f"PASS: verify-proof accepted {verify_response.get('family', 'unknown')} certificate")

teach_whale = json.dumps({'text': 'A whale is a mammal.'})
teach_mammal = json.dumps({'text': 'A mammal is an animal.'})
teach_falcon = json.dumps({'text': 'A falcon is a raptor.'})
teach_raptor = json.dumps({'text': 'A raptor is a bird.'})
teach_bird = json.dumps({'text': 'A bird is an animal.'})
teach_feathers = json.dumps({'text': 'a bird has feathers'})
for payload in (teach_whale, teach_mammal, teach_falcon, teach_raptor, teach_bird, teach_feathers):
  subprocess.check_output([
    'curl', '-sS', '-X', 'POST', f'{base}/teach',
    '-H', 'Content-Type: application/json', '-d', payload,
  ], text=True).strip()

language_query = json.dumps({'text': 'Is a whale an animal?'})
language_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/query',
  '-H', 'Content-Type: application/json', '-d', language_query,
], text=True).strip()
language_response = json.loads(language_out)
language_certificate = language_response.get('certificate')
if not language_certificate or language_certificate.get('domain') != 'language':
  print(f"FAIL: expected language proof certificate, got: {language_response}")
  sys.exit(1)

language_verify_payload = json.dumps({'certificate': language_certificate})
language_verify_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/verify-proof',
  '-H', 'Content-Type: application/json', '-d', language_verify_payload,
], text=True).strip()
language_verify_response = json.loads(language_verify_out)
if not language_verify_response.get('ok'):
  print(f"FAIL: verify-proof rejected language certificate: {language_verify_response}")
  sys.exit(1)
print(f"PASS: verify-proof accepted {language_verify_response.get('family', 'unknown')} language certificate")

deep_relation_query = json.dumps({'text': 'Is a falcon an animal?'})
deep_relation_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/query',
  '-H', 'Content-Type: application/json', '-d', deep_relation_query,
], text=True).strip()
deep_relation_response = json.loads(deep_relation_out)
if deep_relation_response.get('answer') != '[CRYSTAL] YES: a falcon is an animal.':
  print(f"FAIL: deep taught relation did not infer transitively: {deep_relation_response}")
  sys.exit(1)
print('PASS: deep taught relation inferred transitively')

property_guard_query = json.dumps({'text': 'Does a falcon have feathers?'})
property_guard_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/query',
  '-H', 'Content-Type: application/json', '-d', property_guard_query,
], text=True).strip()
property_guard_response = json.loads(property_guard_out)
if property_guard_response.get('answer') != '[CRYSTAL] NO: I cannot establish that a falcon is feathers.':
  print(f"FAIL: has_property leaked transitively when it should stay direct: {property_guard_response}")
  sys.exit(1)
print('PASS: has_property remained direct-only under deep taught hierarchy')

instruction_query = json.dumps({'text': 'How do I restart the server?'})
instruction_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/query',
  '-H', 'Content-Type: application/json', '-d', instruction_query,
], text=True).strip()
instruction_response = json.loads(instruction_out)
instruction_answer = instruction_response.get('answer', '')
instruction_certificate = instruction_response.get('certificate')
if not instruction_answer.startswith('[CRYSTAL] [PLAN]'):
  print(f"FAIL: instruction parse did not return grounded plan: {instruction_response}")
  sys.exit(1)
if not instruction_certificate or instruction_certificate.get('domain') != 'language':
  print(f"FAIL: instruction parse did not return language certificate: {instruction_response}")
  sys.exit(1)

instruction_verify_payload = json.dumps({'certificate': instruction_certificate})
instruction_verify_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/verify-proof',
  '-H', 'Content-Type: application/json', '-d', instruction_verify_payload,
], text=True).strip()
instruction_verify_response = json.loads(instruction_verify_out)
if not instruction_verify_response.get('ok'):
  print(f"FAIL: verify-proof rejected instruction certificate: {instruction_verify_response}")
  sys.exit(1)
print(f"PASS: verify-proof accepted {instruction_verify_response.get('family', 'unknown')} instruction certificate")

execute_allow_payload = json.dumps({'certificate': instruction_certificate, 'action_index': 0})
execute_allow_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/execute-plan',
  '-H', 'Content-Type: application/json', '-d', execute_allow_payload,
], text=True).strip()
execute_allow_response = json.loads(execute_allow_out)
if not execute_allow_response.get('ok') or not execute_allow_response.get('executed'):
  print(f"FAIL: execute-plan rejected safe inspect action: {execute_allow_response}")
  sys.exit(1)
print('PASS: execute-plan accepted safe inspect action')

execute_mutate_payload = json.dumps({'certificate': instruction_certificate, 'action_index': 1})
execute_mutate_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/execute-plan',
  '-H', 'Content-Type: application/json', '-d', execute_mutate_payload,
], text=True).strip()
execute_mutate_response = json.loads(execute_mutate_out)
if execute_mutate_response.get('ok') or not execute_mutate_response.get('requires_approval'):
  print(f"FAIL: execute-plan did not gate mutate action: {execute_mutate_response}")
  sys.exit(1)
print('PASS: execute-plan gated mutate action without approval token')

execute_mutate_approved_payload = json.dumps({
  'certificate': instruction_certificate,
  'action_index': 1,
  'approval_token': os.environ.get('CSIF_EXEC_APPROVAL_TOKEN', 'smoke-approval-token'),
})
execute_mutate_approved_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/execute-plan',
  '-H', 'Content-Type: application/json', '-d', execute_mutate_approved_payload,
], text=True).strip()
execute_mutate_approved_response = json.loads(execute_mutate_approved_out)
if not execute_mutate_approved_response.get('ok') or not execute_mutate_approved_response.get('executed'):
  print(f"FAIL: execute-plan did not allow mutate action with valid approval token: {execute_mutate_approved_response}")
  sys.exit(1)
print('PASS: execute-plan allowed mutate action with valid approval token')

audit_log_path = os.environ.get('CSIF_EXEC_AUDIT_LOG_PATH')
if not audit_log_path:
  print('FAIL: missing audit log path in smoke environment')
  sys.exit(1)

try:
  with open(audit_log_path, 'r', encoding='utf-8') as f:
    audit_events = [json.loads(line) for line in f if line.strip()]
except FileNotFoundError:
  print('FAIL: execute audit log file was not created')
  sys.exit(1)

if len(audit_events) < 3:
  print(f'FAIL: expected at least 3 execute audit events, got {len(audit_events)}')
  sys.exit(1)

has_required_fields = all(
  {'timestamp', 'certificate_family', 'action_hash', 'reason'}.issubset(event.keys())
  for event in audit_events
)
if not has_required_fields:
  print(f'FAIL: execute audit events missing required fields: {audit_events}')
  sys.exit(1)

has_blocked_mutation = any(
  event.get('requires_approval') and event.get('ok') is False
  for event in audit_events
)
if not has_blocked_mutation:
  print(f'FAIL: expected blocked mutation audit event: {audit_events}')
  sys.exit(1)

has_approved_mutation = any(
  event.get('ok') and event.get('executed') and 'approved' in event.get('reason', '')
  for event in audit_events
)
if not has_approved_mutation:
  print(f'FAIL: expected approved mutation audit event: {audit_events}')
  sys.exit(1)

print('PASS: execute audit log captured blocked and approved mutation decisions')

unauthorized_status = subprocess.check_output([
  'curl', '-sS', '-o', '/dev/null', '-w', '%{http_code}',
  f'{base}/admin/execute-audit?limit=2',
], text=True).strip()
if unauthorized_status != '401':
  print(f'FAIL: admin execute-audit should require admin token, got status {unauthorized_status}')
  sys.exit(1)
print('PASS: admin execute-audit rejected unauthorized request')

admin_token = os.environ.get('CSIF_ADMIN_TOKEN', 'smoke-admin-token')
admin_audit_out = subprocess.check_output([
  'curl', '-sS', '-X', 'GET', f'{base}/admin/execute-audit?limit=2',
  '-H', f'x-csif-admin-token: {admin_token}',
], text=True).strip()
admin_audit_response = json.loads(admin_audit_out)
admin_events = admin_audit_response.get('events', [])
if len(admin_events) == 0 or len(admin_events) > 2:
  print(f'FAIL: admin execute-audit tail did not honor limit semantics: {admin_audit_response}')
  sys.exit(1)

family_audit_out = subprocess.check_output([
  'curl', '-sS', '-X', 'GET', f'{base}/admin/execute-audit?family=language-instruction-request&limit=5',
  '-H', f'x-csif-admin-token: {admin_token}',
], text=True).strip()
family_audit_response = json.loads(family_audit_out)
family_events = family_audit_response.get('events', [])
if not family_events or any(event.get('certificate_family') != 'language-instruction-request' for event in family_events):
  print(f'FAIL: admin execute-audit family filter failed: {family_audit_response}')
  sys.exit(1)

print('PASS: admin execute-audit endpoint enforced token and returned tailed/filtered events')

narrative_event_query = json.dumps({'text': 'rain caused flooding'})
narrative_event_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/query',
  '-H', 'Content-Type: application/json', '-d', narrative_event_query,
], text=True).strip()
narrative_event_response = json.loads(narrative_event_out)
narrative_event_answer = narrative_event_response.get('answer', '')
narrative_event_certificate = narrative_event_response.get('certificate')
if not narrative_event_answer.startswith('[CRYSTAL] [TEACHING] Narrative event persisted:'):
  print(f"FAIL: narrative event was not grounded into RWIF: {narrative_event_response}")
  sys.exit(1)
if not narrative_event_certificate or narrative_event_certificate.get('domain') != 'language':
  print(f"FAIL: narrative event did not return language certificate: {narrative_event_response}")
  sys.exit(1)

narrative_event_verify_payload = json.dumps({'certificate': narrative_event_certificate})
narrative_event_verify_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/verify-proof',
  '-H', 'Content-Type: application/json', '-d', narrative_event_verify_payload,
], text=True).strip()
narrative_event_verify_response = json.loads(narrative_event_verify_out)
if not narrative_event_verify_response.get('ok'):
  print(f"FAIL: verify-proof rejected narrative event certificate: {narrative_event_verify_response}")
  sys.exit(1)
print(f"PASS: verify-proof accepted {narrative_event_verify_response.get('family', 'unknown')} narrative event certificate")

narrative_state_query = json.dumps({'text': 'The server is not responding.'})
narrative_state_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/query',
  '-H', 'Content-Type: application/json', '-d', narrative_state_query,
], text=True).strip()
narrative_state_response = json.loads(narrative_state_out)
if not narrative_state_response.get('answer', '').startswith('[CRYSTAL] [TEACHING] Narrative state persisted:'):
  print(f"FAIL: narrative state was not grounded into RWIF: {narrative_state_response}")
  sys.exit(1)
print('PASS: narrative state persisted pre-restart')

ambiguous_query = json.dumps({'text': 'Restart the server'})
ambiguous_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/query',
  '-H', 'Content-Type: application/json', '-d', ambiguous_query,
], text=True).strip()
ambiguous_response = json.loads(ambiguous_out)
if not ambiguous_response.get('answer', '').startswith('[NEEDS_INPUT] Do you want me to treat this as an instruction'):
  print(f"FAIL: ambiguous imperative did not trigger clarification: {ambiguous_response}")
  sys.exit(1)
print('PASS: ambiguous imperative triggered clarification')

# Negative hardening: tamper the certificate and ensure verifier rejects it.
tampered = json.loads(json.dumps(certificate))
tampered['payload']['result_points'] = [{'num': 1, 'den': 1}]
tampered_payload = json.dumps({'certificate': tampered})
tampered_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/verify-proof',
  '-H', 'Content-Type: application/json', '-d', tampered_payload,
], text=True).strip()
tampered_response = json.loads(tampered_out)
if tampered_response.get('ok'):
  print(f"FAIL: verify-proof accepted tampered certificate: {tampered_response}")
  sys.exit(1)
print('PASS: verify-proof rejected tampered certificate')

print('Math attack smoke: PASS')
PY

kill "$SERVER_PID" 2>/dev/null || true
wait "$SERVER_PID" 2>/dev/null || true
unset SERVER_PID

start_server
wait_for_server

MATH_ATTACKS_PORT="$PORT" MATH_ATTACKS_BANK_PATH_RUNTIME="$BANK_PATH" python3 - <<'PY'
import json
import os
import subprocess
import sys

base = f"http://127.0.0.1:{os.environ.get('MATH_ATTACKS_PORT', '18182')}"
bank_path = os.environ['MATH_ATTACKS_BANK_PATH_RUNTIME']

post_restart_queries = [
  ('Is a whale an animal?', '[CRYSTAL] YES: a whale is an animal.'),
  ('Is a falcon an animal?', '[CRYSTAL] YES: a falcon is an animal.'),
]

for query, expected in post_restart_queries:
  payload = json.dumps({'text': query})
  out = subprocess.check_output([
    'curl', '-sS', '-X', 'POST', f'{base}/query',
    '-H', 'Content-Type: application/json', '-d', payload,
  ], text=True).strip()
  response = json.loads(out)
  if response.get('answer') != expected:
    print(f"FAIL: post-restart memory regression for '{query}': {response}")
    sys.exit(1)
print('PASS: taught relations persisted across restart')

# Add one more narrative write post-restart so trajectory reflects both sessions.
state_payload = json.dumps({'text': 'The server is not responding.'})
state_out = subprocess.check_output([
  'curl', '-sS', '-X', 'POST', f'{base}/query',
  '-H', 'Content-Type: application/json', '-d', state_payload,
], text=True).strip()
state_response = json.loads(state_out)
if not state_response.get('answer', '').startswith('[CRYSTAL] [TEACHING] Narrative state persisted:'):
  print(f"FAIL: post-restart narrative state write failed: {state_response}")
  sys.exit(1)

with open(bank_path, 'r', encoding='utf-8') as f:
  crystal = json.load(f)

edges = crystal.get('edges', {})
state_edge_ok = any(
  edge.get('relation') == 'state_not_at' and len(edge.get('trajectory', [])) >= 2
  for edge in edges.values()
)
if not state_edge_ok:
  print('FAIL: expected persisted state_not_at trajectory across restart')
  sys.exit(1)

observed_edge_ok = any(edge.get('relation') == 'observed_at' for edge in edges.values())
if not observed_edge_ok:
  print('FAIL: expected observed_at temporal anchors in persisted bank')
  sys.exit(1)

print('PASS: narrative temporal persistence survived restart with trajectory continuity')
print('Math attack restart persistence: PASS')
PY
