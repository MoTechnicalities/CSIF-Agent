# CSIF-Agent Operations

## 1) Install as user-level systemd service

Run:

```bash
scripts/install_user_service.sh
```

This installs:

- Unit: `~/.config/systemd/user/csif-agent.service`
- Env file: `~/.config/csif-agent/csif-agent.env`

Update secrets/tokens in the env file after first install.

Useful commands:

```bash
systemctl --user daemon-reload
systemctl --user enable --now csif-agent.service
systemctl --user restart csif-agent.service
systemctl --user status csif-agent.service
journalctl --user -u csif-agent.service -f
```

## 2) Run the production probe

```bash
CSIF_PROBE_BASE_URL=http://127.0.0.1:19191 \
CSIF_EXEC_APPROVAL_TOKEN=<your-approval-token> \
CSIF_ADMIN_TOKEN=<your-admin-token> \
scripts/probe_production.py
```

Probe verifies:

- `/health`
- `/query` math solve + math certificate
- `/verify-proof` accept/reject behavior
- `/execute-plan` inspect allow + mutate gate + mutate approval
- `/admin/execute-audit` unauthorized/authorized behavior (when admin token set)

## 3) Runtime artifacts

Default runtime files in repo root under `.runtime/`:

- `deploy_bank.rwif`
- `execute_audit.jsonl`
- `deploy_server.log`
