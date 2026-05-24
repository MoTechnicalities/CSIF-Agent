# CSIF-Agent Throughput Benchmark (2026-05-24)

## Objective
Measure CSIF-Agent request throughput and latency on non-GPU consumer hardware against a broad query battery.

## Environment
- Host CPU: Intel(R) Core(TM) i5-4460 CPU @ 3.20GHz
- Host logical cores: 4
- Host memory: 11Gi
- Swap: 2.0Gi (fully used at snapshot time)
- Runtime: Docker container `csif-agent`
- Container image: `sha256:3093460ebeeb59d203e017419766837a8dbb28d8fd480d3c426f6744801f140e`
- Health at run time: `healthy`

## Concurrent System Load Context
- Snapshot recorded from the same host during the benchmark session in `docs/benchmarks/container_load_snapshot_2026-05-24.txt`.
- Active containers at snapshot: 20 total (including CSIF-Agent plus Home Assistant, Open-WebUI, two Homebridge instances, two n8n instances, Jellyfin, Navidrome, Mosquitto, Joomla app+db, Ring MQTT, Matter server, Piper, OpenClaw, OceanEco app+db, Caddy, DDClient).
- Aggregate Docker memory usage at snapshot: ~2.274 GiB.
- Host memory at snapshot: 11Gi total, 5.8Gi used, 673Mi free, 5.8Gi available.
- Host swap at snapshot: 2.0Gi used of 2.0Gi.
- Host load average at snapshot: 0.97 (1m), 0.80 (5m), 0.99 (15m).

## Battery
- Source: `data/base_lobe_v1/benchmarks/base_lobe_v1_benchmark.jsonl`
- Query-only subset used for throughput: 555 requests per run
- Category distribution (query requests):
  - `taxonomy`: 90
  - `properties`: 135
  - `transitive`: 120
  - `causality`: 45
  - `arithmetic`: 120
  - `honesty`: 45

## Method
1. Restart container once before the benchmark to capture a cold run.
2. Execute 5 sequential runs of the 555-query battery against `http://127.0.0.1:8080/query`.
3. Record per-run throughput (QPS) and latency percentiles (p50/p95/p99/max).
4. Report:
   - Run 1 as cold-cache behavior.
   - Runs 2-5 as warm-cache behavior.

## Results

### Per-Run Throughput and Latency
| Run | Cache | Requests | Throughput (QPS) | p50 (ms) | p95 (ms) | p99 (ms) | max (ms) |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | cold | 555 | 61.34 | 10.51 | 48.01 | 70.25 | 71.47 |
| 2 | warm | 555 | 60.26 | 10.66 | 49.35 | 72.23 | 86.50 |
| 3 | warm | 555 | 60.13 | 10.65 | 49.09 | 71.43 | 73.94 |
| 4 | warm | 555 | 61.69 | 10.50 | 49.09 | 70.66 | 89.48 |
| 5 | warm | 555 | 62.77 | 10.17 | 47.19 | 69.61 | 83.60 |

### Warm-Run Summary (Runs 2-5)
- Mean throughput: **61.21 QPS**
- Throughput range: **60.13 to 62.77 QPS**
- Mean p95 latency: **48.68 ms**

### Warm-Run Category-Level Performance (mean of runs 2-5)
| Category | Mean latency (ms) | p95 latency (ms) | QPS estimate |
|---|---:|---:|---:|
| arithmetic | 1.83 | 2.78 | 547.27 |
| causality | 9.06 | 11.22 | 110.43 |
| honesty | 6.71 | 8.99 | 149.13 |
| properties | 10.84 | 13.25 | 92.31 |
| taxonomy | 11.53 | 13.38 | 86.78 |
| transitive | 46.61 | 70.69 | 21.46 |

## Interpretation
- The system maintains stable overall throughput around **~61 QPS** on CPU-only hardware.
- Arithmetic and direct factual queries are very fast.
- Transitive inference is the dominant cost center and determines the long tail of latency.
- These results were captured under multi-service operational load rather than on an isolated benchmark host, providing a conservative and fair real-world baseline.

## Artifacts
- Raw benchmark output (JSON): `docs/benchmarks/csif_throughput_2026-05-24.json`
- Runtime load snapshot (text): `docs/benchmarks/container_load_snapshot_2026-05-24.txt`

## Reproduction Notes
- Endpoint: `http://127.0.0.1:8080/query`
- Benchmark style: sequential single-client requests (no load-generator concurrency)
- For concurrent stress/load testing, add a multi-client driver in a separate benchmark profile.
