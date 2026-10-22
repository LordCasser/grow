"""AB/BA release comparison using retained binaries; never builds during timing."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

p = argparse.ArgumentParser()
p.add_argument("before", type=Path)
p.add_argument("after", type=Path)
p.add_argument("--output", type=Path, required=True)
a = p.parse_args()
openspec = next(parent for parent in Path(__file__).resolve().parents if parent.name == "openspec")
scripts = list((openspec / "changes").glob("profile-release-resume-first-history/run_matrix.py"))
scripts += list((openspec / "changes/archive").glob("*-profile-release-resume-first-history/run_matrix.py"))
if len(scripts) != 1:
    raise SystemExit(f"expected one retained matrix runner, found {scripts}")
script = scripts[0]
measurements = {"before": [], "after": []}
for repeat in range(1, 4):
    for stage in (["before", "after"] if repeat % 2 else ["after", "before"]):
        label = f"paired-{stage}-{repeat}"
        subprocess.run([sys.executable, str(script), str(getattr(a, stage).resolve()),
                        "--output", str(a.output), "--stage", label, "--runs", "1"], check=True)
        rows = json.loads((a.output / f"{label}-measurements.json").read_text())
        for row in rows:
            row["run"] = repeat
            row["stage"] = stage
        measurements[stage].extend(rows)
        (a.output / f"paired-{stage}-measurements.json").write_text(json.dumps(measurements[stage], indent=2) + "\n")
