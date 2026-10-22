"""Run built scoped tests, retaining failures and rejecting empty selections."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

GROUPS = {
    "shell": [
        "session::history::tests", "session::storage::jsonl::", "session::persistence::",
        "session::trajectory::", "session::image_describe::", "session::storage::search::",
        "session::storage::tests", "session::storage::communication_history::",
        "session::storage::transcript::", "agent::mvp_agent::tests", "agent::subagent::tests",
        "extensions::session_state::tests", "extensions::session_updates::tests",
        "session::notifications::tests", "session::goal_notification::tests",
        "session::actor::tests", "session::actor::updates::", "session::actor::spawn::",
        "tools::notification_bridge::tests",
    ],
    "pager": [
        "app::acp_handler::tests::", "acp::tracker::tests::",
        "transcript_projection::", "replay_cmd::", "export_cmd::tests",
        "trajectory_cmd::tests", "app::subagent::",
        "app::root::dispatch::tests::session::load::",
        "app::root::dispatch::tests::session::fork::",
    ],
}

p = argparse.ArgumentParser()
p.add_argument("binary", type=Path)
p.add_argument("--kind", choices=GROUPS, required=True)
p.add_argument("--output", type=Path, required=True)
p.add_argument("--stage", required=True)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
env = os.environ.copy()
env["RUST_MIN_STACK"] = "16777216"
results = []
for group in GROUPS[a.kind]:
    path = a.output / (a.stage + "-" + group.replace("::", "-").strip("-") + ".log")
    try:
        with path.open("w") as out:
            run = subprocess.run([str(a.binary.resolve()), group, "--test-threads=1", "--nocapture"],
                                 env=env, stdout=out, stderr=subprocess.STDOUT, timeout=900)
        summaries = list(re.finditer(r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored", path.read_text(), re.MULTILINE))
        match = summaries[-1] if summaries else None
        row = {"filter": group, "exit": run.returncode, "log": path.name,
               "passed": int(match[2]) if match else 0,
               "failed": int(match[3]) if match else None,
               "ignored": int(match[4]) if match else None}
        row["ok"] = run.returncode == 0 and row["passed"] > 0
    except subprocess.TimeoutExpired:
        row = {"filter": group, "ok": False, "timeout": True, "log": path.name}
    results.append(row)
    print(json.dumps(row), flush=True)
    (a.output / f"{a.stage}-{a.kind}-results.json").write_text(json.dumps(results, indent=2) + "\n")
raise SystemExit(0 if all(row["ok"] for row in results) else 1)
