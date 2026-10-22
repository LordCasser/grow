"""Pinned release source-completion controls, with no CI time thresholds."""
import hashlib
import json
import os
import re
import statistics
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = next(p for p in HERE.parents if (p / "Cargo.toml").is_file())
OUT = HERE / "micro"
OUT.mkdir(exist_ok=True)
BINS = {stage: (ROOT / f"target/tui-scan-release/binaries/{stage}-pager").resolve()
        for stage in ("before", "after")}
TEST = "scrollback::blocks::source_scan_tests::source_scan_deferred_finish_microbenchmark"
(OUT / "identities.json").write_text(json.dumps({k: {"path": str(v), "sha256": hashlib.sha256(v.read_bytes()).hexdigest()} for k, v in BINS.items()}, indent=2) + "\n")
rows = []
for run in range(4):  # 0 is a separate warmup, never counted in medians.
    for stage in (["before", "after"] if run % 2 else ["after", "before"]):
        log = OUT / f"{stage}-{run}.log"
        env = {k: v for k, v in os.environ.items() if not k.startswith("GROW_")}
        env.update(RUST_MIN_STACK="16777216", SOURCE_SCAN_BENCH_ITERATIONS="32" if run == 0 else "512")
        with log.open("w") as output:
            result = subprocess.run([str(BINS[stage]), TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"], cwd=ROOT / "crates/codegen/pager", env=env, stdout=output, stderr=subprocess.STDOUT, timeout=300)
        text = log.read_text()
        assert result.returncode == 0 and "test result: ok. 1 passed;" in text, log
        matches = re.findall(r"\[source-scan\] case=(\S+) source_bytes=(\d+) messages=(\d+) elapsed_ms=([\d.]+)", text)
        assert len(matches) == 4, log
        if run:
            rows.extend(dict(stage=stage, run=run, case=case, source_bytes=int(size), messages=int(count), elapsed_ms=float(ms), log=log.name) for case, size, count, ms in matches)
        (OUT / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
        print(stage, run, "passed", flush=True)
summary = []
for case in sorted({r["case"] for r in rows}):
    for stage in BINS:
        selected = [r for r in rows if r["stage"] == stage and r["case"] == case]
        values = [r["elapsed_ms"] for r in selected]
        summary.append(dict(case=case, stage=stage, n=len(values), messages=selected[0]["messages"], source_bytes=selected[0]["source_bytes"], median_ms=statistics.median(values), range_ms=[min(values), max(values)]))
(OUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
