"""Run a built release test; fixture/build/oracle work is outside load timing."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
from statistics import median
import subprocess

PROFILES = [
    ("modern-128", 128, 8, 0, False),
    ("modern-512", 512, 8, 0, False),
    ("modern-2048", 2048, 8, 0, False),
    ("modern-repair-512", 512, 8, 16, False),
    ("modern-short-512", 512, 1, 0, False),
]


def main():
    p = argparse.ArgumentParser()
    p.add_argument("binary", type=Path)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--stage", required=True)
    p.add_argument("--runs", type=int, default=3)
    p.add_argument("--correctness", action="store_true")
    p.add_argument("--profiles", nargs="*")
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    binary = args.binary.resolve()
    profiles = list(PROFILES)
    if args.correctness:
        profiles = [PROFILES[0], PROFILES[2], PROFILES[3],
                    ("modern-route-512", 512, 8, 0, True)]
    if args.profiles:
        profiles = [row for row in profiles if row[0] in args.profiles]
    if not profiles:
        p.error("no profiles selected")
    suffix = "-correctness" if args.correctness else ""
    stage = args.stage + suffix
    metadata = {"binary": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "profile": "release", "fixture": "cold process; warm page cache; setup outside RPC",
                "rss_scope": "whole test process including fixture, not isolated load peak"}
    (args.output / f"{stage}-binary.json").write_text(json.dumps(metadata, indent=2) + "\n")
    measurements = []
    for repeat in range(1, (1 if args.correctness else args.runs) + 1):
        for name, turns, chunks, missing, omit_route in profiles:
            label = f"{stage}-{name}-{repeat}"
            env = {k: v for k, v in os.environ.items() if not k.startswith("GROW_")}
            env.update({"RUST_MIN_STACK": "16777216", "GROW_PERF_SESSION_FORMAT": "modern",
                        "GROW_PERF_TURNS": str(turns), "GROW_PERF_AGENT_CHUNKS_PER_TURN": str(chunks),
                        "GROW_PERF_AGENT_CHUNK_LEN": "4096",
                        "GROW_PERF_TRACE_PATH": str((args.output / f"{label}.trace.jsonl").resolve()),
                        "GROW_PERF_MISSING_PROJECTION_EVERY": str(missing)})
            if omit_route:
                env["GROW_PERF_OMIT_ROUTE_BASELINE"] = "1"
            if args.correctness:
                env["GROW_PERF_ASSERT_HISTORY"] = "1"
            command = ["/usr/bin/time", "-l", str(binary), "full_session_load_e2e", "--exact",
                       "--ignored", "--nocapture", "--test-threads=1"]
            log_path = args.output / f"{label}.txt"
            with log_path.open("w") as log:
                result = subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=300)
            if result.returncode:
                raise SystemExit(f"{label}: exit {result.returncode}; inspect {log_path}")
            contents = log_path.read_text()
            rows = [json.loads(line[len("PERF_RESUME "):]) for line in contents.splitlines()
                    if line.startswith("PERF_RESUME ")]
            if len(rows) != 1:
                raise SystemExit(f"{label}: expected exactly one PERF_RESUME record")
            row = rows[0]
            rss = re.search(r"(\d+)\s+maximum resident set size", contents)
            row.update(profile=name, run=repeat, stage=stage, source=log_path.name,
                       process_peak_rss_bytes=int(rss[1]) if rss else None)
            measurements.append(row)
            (args.output / f"{stage}-measurements.json").write_text(json.dumps(measurements, indent=2) + "\n")
            print(f"{label}: first={row['first_history_ms']:.1f} last={row['last_history_ms']:.1f} "
                  f"load={row['load_ms']:.1f} ms", flush=True)
    summary = []
    for name, *_ in profiles:
        rows = [row for row in measurements if row["profile"] == name]
        item = {"profile": name, "runs": len(rows)}
        for key in ["load_ms", "first_history_ms", "last_history_ms", "max_history_gap_ms",
                    "process_peak_rss_bytes", "timeline_bytes", "updates_bytes", "directory_bytes", "directory_files"]:
            values = [row[key] for row in rows if row.get(key) is not None]
            if values:
                item[key] = median(values)
                item[key + "_range"] = [min(values), max(values)]
        phases = sorted({p["name"] for row in rows for p in row["phases"]})
        item["phase_medians_ms"] = {name: median(sum(p["ms"] for p in row["phases"] if p["name"] == name)
                                                for row in rows) for name in phases}
        summary.append(item)
    (args.output / f"{stage}-summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    main()
