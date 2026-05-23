#!/usr/bin/env python3
"""Post-deploy probe for CSIF-Agent production endpoints.

Checks:
- /health
- /query + /verify-proof (math)
- /query + /verify-proof + /execute-plan (language instruction)
- /execute-plan mutate gating and approval token override
- /admin/execute-audit auth and retrieval when admin token is provided
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Optional


@dataclass
class ProbeResult:
    name: str
    ok: bool
    detail: str = ""


def http_json(
    method: str,
    url: str,
    payload: Optional[dict[str, Any]] = None,
    headers: Optional[dict[str, str]] = None,
) -> tuple[int, str, Optional[dict[str, Any]]]:
    body = None
    req_headers = {"Content-Type": "application/json"}
    if headers:
        req_headers.update(headers)
    if payload is not None:
        body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, method=method, data=body, headers=req_headers)
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            raw = resp.read().decode("utf-8")
            parsed = None
            if raw.strip():
                try:
                    parsed = json.loads(raw)
                except json.JSONDecodeError:
                    parsed = None
            return resp.status, raw, parsed
    except urllib.error.HTTPError as err:
        raw = err.read().decode("utf-8") if err.fp else ""
        parsed = None
        if raw.strip():
            try:
                parsed = json.loads(raw)
            except json.JSONDecodeError:
                parsed = None
        return err.code, raw, parsed


def require(results: list[ProbeResult], name: str, condition: bool, detail: str = "") -> None:
    results.append(ProbeResult(name=name, ok=condition, detail=detail))


def main() -> int:
    base_url = os.environ.get("CSIF_PROBE_BASE_URL", "http://127.0.0.1:19191").rstrip("/")
    approval_token = os.environ.get("CSIF_EXEC_APPROVAL_TOKEN")
    admin_token = os.environ.get("CSIF_ADMIN_TOKEN")

    results: list[ProbeResult] = []

    health_status, health_raw, _ = http_json("GET", f"{base_url}/health", payload=None, headers={})
    require(
        results,
        "health",
        health_status == 200 and health_raw.strip() == "ok",
        f"status={health_status} body={health_raw.strip()}",
    )

    q_status, _, q_payload = http_json("POST", f"{base_url}/query", {"text": "solve (x+1)/(x-1) = 0"})
    q_answer = (q_payload or {}).get("answer", "")
    math_cert = (q_payload or {}).get("certificate")
    require(results, "query_math", q_status == 200 and q_answer.startswith("[CRYSTAL] [SOLVE]"), q_answer)
    require(
        results,
        "query_math_certificate",
        isinstance(math_cert, dict) and math_cert.get("domain") == "math",
        str(math_cert),
    )

    verify_ok = False
    if isinstance(math_cert, dict):
        v_status, _, v_payload = http_json(
            "POST", f"{base_url}/verify-proof", {"certificate": math_cert}
        )
        verify_ok = v_status == 200 and bool((v_payload or {}).get("ok"))
        require(results, "verify_math", verify_ok, str(v_payload))

        tampered = json.loads(json.dumps(math_cert))
        tampered.setdefault("payload", {})["result_points"] = [{"num": 1, "den": 1}]
        t_status, _, t_payload = http_json(
            "POST", f"{base_url}/verify-proof", {"certificate": tampered}
        )
        require(
            results,
            "verify_math_tamper",
            t_status == 200 and not bool((t_payload or {}).get("ok")),
            str(t_payload),
        )

    i_status, _, i_payload = http_json(
        "POST", f"{base_url}/query", {"text": "How do I restart the server?"}
    )
    i_answer = (i_payload or {}).get("answer", "")
    lang_cert = (i_payload or {}).get("certificate")
    require(
        results,
        "query_instruction",
        i_status == 200 and i_answer.startswith("[CRYSTAL] [PLAN]"),
        i_answer,
    )
    require(
        results,
        "query_instruction_certificate",
        isinstance(lang_cert, dict) and lang_cert.get("domain") == "language",
        str(lang_cert),
    )

    if isinstance(lang_cert, dict):
        lv_status, _, lv_payload = http_json(
            "POST", f"{base_url}/verify-proof", {"certificate": lang_cert}
        )
        require(
            results,
            "verify_instruction",
            lv_status == 200 and bool((lv_payload or {}).get("ok")),
            str(lv_payload),
        )

        ex_ok_status, _, ex_ok_payload = http_json(
            "POST",
            f"{base_url}/execute-plan",
            {"certificate": lang_cert, "action_index": 0},
        )
        require(
            results,
            "execute_inspect",
            ex_ok_status == 200
            and bool((ex_ok_payload or {}).get("ok"))
            and bool((ex_ok_payload or {}).get("executed")),
            str(ex_ok_payload),
        )

        ex_mut_status, _, ex_mut_payload = http_json(
            "POST",
            f"{base_url}/execute-plan",
            {"certificate": lang_cert, "action_index": 1},
        )
        require(
            results,
            "execute_mutate_gated",
            ex_mut_status == 200
            and not bool((ex_mut_payload or {}).get("ok"))
            and bool((ex_mut_payload or {}).get("requires_approval")),
            str(ex_mut_payload),
        )

        if approval_token:
            ex_app_status, _, ex_app_payload = http_json(
                "POST",
                f"{base_url}/execute-plan",
                {
                    "certificate": lang_cert,
                    "action_index": 1,
                    "approval_token": approval_token,
                },
            )
            require(
                results,
                "execute_mutate_approved",
                ex_app_status == 200
                and bool((ex_app_payload or {}).get("ok"))
                and bool((ex_app_payload or {}).get("executed")),
                str(ex_app_payload),
            )

    if admin_token:
        unauth_status, _, _ = http_json(
            "GET", f"{base_url}/admin/execute-audit?limit=2", payload=None, headers={}
        )
        require(
            results,
            "admin_audit_unauthorized",
            unauth_status == 401,
            f"status={unauth_status}",
        )

        auth_status, _, auth_payload = http_json(
            "GET",
            f"{base_url}/admin/execute-audit?limit=5",
            payload=None,
            headers={"x-csif-admin-token": admin_token},
        )
        events = (auth_payload or {}).get("events") if isinstance(auth_payload, dict) else None
        require(
            results,
            "admin_audit_authorized",
            auth_status == 200 and isinstance(events, list),
            str(auth_payload),
        )

    failed = [r for r in results if not r.ok]
    for r in results:
        status = "PASS" if r.ok else "FAIL"
        detail = f" :: {r.detail}" if r.detail else ""
        print(f"{status}: {r.name}{detail}")

    print("\nOverall:", "PASS" if not failed else "FAIL")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
