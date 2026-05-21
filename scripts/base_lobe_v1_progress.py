#!/usr/bin/env python3

from pathlib import Path
import json

ROOT = Path(__file__).resolve().parents[1]
SEED_DIR = ROOT / "data" / "base_lobe_v1" / "seed"
BENCHMARK_PATH = ROOT / "data" / "base_lobe_v1" / "benchmarks" / "base_lobe_v1_benchmark.jsonl"

TARGETS = {
    "taxonomy": 8000,
    "causality": 2500,
    "properties": 3000,
    "geography": 2000,
    "arithmetic": 1500,
    "operator_utility": 1000,
}


def count_non_empty_lines(path: Path) -> int:
    if not path.exists():
        return 0
    return sum(1 for line in path.read_text(encoding="utf-8").splitlines() if line.strip())


def load_benchmark_counts(path: Path):
    out = {}
    if not path.exists():
        return out
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        item = json.loads(line)
        cat = item.get("category", "uncategorized")
        out[cat] = out.get(cat, 0) + 1
    return out


def main():
    print("Base Lobe v1 Progress")
    print("=====================")

    total_current = 0
    total_target = sum(TARGETS.values())
    for category, target in TARGETS.items():
        current = count_non_empty_lines(SEED_DIR / f"{category}.txt")
        total_current += current
        pct = 0.0 if target == 0 else (current / target) * 100.0
        print(f"{category:16} {current:6d}/{target:<6d} ({pct:5.1f}%)")

    print("---------------------")
    total_pct = 0.0 if total_target == 0 else (total_current / total_target) * 100.0
    print(f"total facts       {total_current:6d}/{total_target:<6d} ({total_pct:5.1f}%)")

    bench_counts = load_benchmark_counts(BENCHMARK_PATH)
    bench_total = sum(bench_counts.values())
    print("\nBenchmark composition")
    print("=====================")
    for cat in sorted(bench_counts):
        print(f"{cat:16} {bench_counts[cat]:6d}")
    print(f"total checks      {bench_total:6d}")


if __name__ == "__main__":
    main()
