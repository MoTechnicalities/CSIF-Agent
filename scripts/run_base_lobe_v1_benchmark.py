#!/usr/bin/env python3

import json
import os
import sys
import urllib.request
import urllib.error
import time
import re
from decimal import Decimal, InvalidOperation

AGENT_URL = "http://localhost:18080"
SUMMARY_JSON = None
VERBOSE = os.environ.get("BENCHMARK_VERBOSE", "0") == "1"
HTTP_TIMEOUT = float(os.environ.get("BENCHMARK_HTTP_TIMEOUT", "3.0"))
HTTP_RETRIES = int(os.environ.get("BENCHMARK_HTTP_RETRIES", "3"))
if len(sys.argv) > 1:
    AGENT_URL = sys.argv[1].rstrip("/")
if len(sys.argv) > 2:
    SUMMARY_JSON = sys.argv[2]

benchmark_path = os.environ.get(
    "BENCHMARK_PATH",
    "data/base_lobe_v1/benchmarks/base_lobe_v1_benchmark.jsonl",
)

ARITHMETIC_TOLERANCE = Decimal("1e-9")


def post_json(path: str, payload: dict) -> dict:
    data = json.dumps(payload).encode("utf-8")
    last_err = None
    for attempt in range(1, HTTP_RETRIES + 1):
        try:
            req = urllib.request.Request(
                f"{AGENT_URL}{path}",
                data=data,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except (urllib.error.URLError, TimeoutError, ConnectionError) as err:
            last_err = err
            if attempt < HTTP_RETRIES:
                time.sleep(0.1)

    raise RuntimeError(f"request failed after {HTTP_RETRIES} attempts: {last_err}")


def _extract_trailing_numeric_result(answer: str):
    match = re.search(r"=\s*([-+]?\d+(?:\.\d+)?)\s*$", answer.strip())
    if not match:
        return None
    try:
        return Decimal(match.group(1))
    except InvalidOperation:
        return None


def _extract_first_numeric_token(text: str):
    match = re.search(r"[-+]?\d+(?:\.\d+)?", text)
    if not match:
        return None
    try:
        return Decimal(match.group(0))
    except InvalidOperation:
        return None


def arithmetic_match(case: dict, answer: str) -> bool:
    actual = _extract_trailing_numeric_result(answer)
    if actual is None:
        return False

    if case.get("expected_mode") == "contains":
        expected = _extract_first_numeric_token(case.get("expected", ""))
        if expected is None:
            return False
        return abs(actual - expected) <= ARITHMETIC_TOLERANCE

    if case.get("expected_mode") == "contains_any":
        for token in case.get("expected_any", []):
            expected = _extract_first_numeric_token(token)
            if expected is not None and abs(actual - expected) <= ARITHMETIC_TOLERANCE:
                return True
        return False

    return False


passed = 0
failed = 0
category_stats = {}

with open(benchmark_path, "r", encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue

        case = json.loads(line)
        case_id = case["id"]
        case_type = case["type"]
        expected_mode = case["expected_mode"]
        category = case.get("category", "uncategorized")

        if category not in category_stats:
            category_stats[category] = {"pass": 0, "fail": 0}

        if case_type == "query":
            try:
                result = post_json("/query", {"text": case["query"]})
                answer = result.get("answer", "")
            except Exception as err:
                answer = f"[ERROR] {err}"
        elif case_type == "teach":
            try:
                result = post_json("/teach", {"text": case["teach"]})
                answer = result.get("answer", "")
            except Exception as err:
                answer = f"[ERROR] {err}"
        else:
            failed += 1
            print(f"FAIL {case_id}: unsupported case type {case_type}")
            continue

        ok = False
        if category == "arithmetic":
            ok = arithmetic_match(case, answer)
        elif expected_mode == "contains":
            ok = case["expected"] in answer
        elif expected_mode == "contains_any":
            ok = any(token in answer for token in case["expected_any"])

        if ok:
            passed += 1
            category_stats[category]["pass"] += 1
            if VERBOSE:
                print(f"PASS {case_id}: {answer}")
        else:
            failed += 1
            category_stats[category]["fail"] += 1
            if VERBOSE:
                print(f"FAIL {case_id}: got={answer}")

print("\nBenchmark summary")
print(f"  passed: {passed}")
print(f"  failed: {failed}")
print("\nCategory summary")
for category in sorted(category_stats):
    cp = category_stats[category]["pass"]
    cf = category_stats[category]["fail"]
    total = cp + cf
    pct = 0.0 if total == 0 else (cp / total) * 100.0
    print(f"  {category}: {cp}/{total} ({pct:.1f}%)")

if SUMMARY_JSON:
    payload = {
        "agent_url": AGENT_URL,
        "passed": passed,
        "failed": failed,
        "total": passed + failed,
        "categories": category_stats,
    }
    with open(SUMMARY_JSON, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2)
        f.write("\n")

if failed > 0:
    sys.exit(1)
