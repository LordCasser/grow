"""Compare the retained response commit probes without overlapping builds."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

p = argparse.ArgumentParser()
p.add_argument("before", type=Path)
p.add_argument("after", type=Path)
p.add_argument("--output", type=Path, required=True)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
binaries = {key: getattr(a, key).resolve() for key in ("before", "after")}
(a.output / "paired-commit-binaries.json").write_text(json.dumps({
    key: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    for key, path in binaries.items()
}, indent=2) + "\n")
measurements = []
for repeat in range(1, 4):
    for stage in (["before", "after"] if repeat % 2 else ["after", "before"]):
        path = a.output / f"paired-{stage}-commit-{repeat}.txt"
        env = os.environ.copy()
        env["RUST_MIN_STACK"] = "16777216"
        with path.open("w") as log:
            run = subprocess.run([
                str(binaries[stage]),
                "session::storage::jsonl::response_commit_perf::normal_response_storage_commit_release",
                "--exact", "--ignored", "--nocapture", "--test-threads=1",
            ], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=300)
        contents = path.read_text()
        rows = [json.loads(line.split("PERF_RESPONSE_COMMIT ", 1)[1])
                for line in contents.splitlines() if "PERF_RESPONSE_COMMIT " in line]
        if run.returncode or len(rows) != 2 or not re.search(r"test result: ok\. 1 passed;", contents):
            raise SystemExit(f"failed or empty: {path}")
        for row in rows:
            row.update(stage=stage, run=repeat, source=path.name)
        measurements.extend(rows)
        (a.output / "paired-commit-measurements.json").write_text(json.dumps(measurements, indent=2) + "\n")
        print(f"{stage} commit {repeat}: passed", flush=True)
