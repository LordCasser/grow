"""Read retained P2 release evidence; no process/benchmark execution."""

import datetime
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = next(p.parent for p in HERE.parents if p.name == "openspec")
P2 = ROOT / "openspec/changes/archive/2026-10-22-reuse-validated-descendant-recovery"


def timestamp_ms(row):
    return datetime.datetime.fromisoformat(row["timestamp"].replace("Z", "+00:00")).timestamp() * 1000


def start_ms(row):
    return timestamp_ms(row) - row["fields"]["elapsed_us"] / 1000


proxies = []
for run in range(1, 4):
    paths = list((P2 / f"pty-log/after-modern-{run}").glob("*2048*-loading.json"))
    assert len(paths) == 1, paths
    path = paths[0]
    raw = json.loads(path.read_text())
    rows = [r for r in raw["loading"]["instrumentation_prefix_through_completion"]
            if r.get("fields", {}).get("event") == "timing"]

    def named(name):
        return [r for r in rows if r["fields"].get("name") == name]

    origin = start_ms(named("session.load_session")[0])
    projection_end = timestamp_ms(named("session.load.history_projection")[0])
    batch = min((r for r in named("pager.acp.apply_batch")
                 if start_ms(r) >= projection_end), key=start_ms)
    present = min((r for r in named("pager.frame.present")
                   if start_ms(r) >= start_ms(batch)), key=start_ms)
    batch_times = sorted(r["fields"]["elapsed_us"] / 1000
                         for r in named("pager.acp.apply_batch"))
    proxies.append({
        "source": str(path.relative_to(ROOT)),
        "projection_end_from_load_ms": projection_end - origin,
        "first_post_projection_batch_start_ms": start_ms(batch) - origin,
        "next_present_start_ms": start_ms(present) - origin,
        "batch_to_present_start_gap_ms": start_ms(present) - start_ms(batch),
        "present_duration_ms": present["fields"]["elapsed_us"] / 1000,
        "batch_count": len(batch_times),
        "batch_max_ms": max(batch_times),
        "batch_p95_nearest_lower_ms": batch_times[int(.95 * (len(batch_times) - 1))],
    })

hotspots = [
    "shell/src/session/history/mod.rs", "shell/src/session/history/projector.rs",
    "shell/src/session/storage/jsonl/ledger.rs", "pager/src/app/root/event_loop.rs",
    "pager/src/scrollback/blocks/agent.rs", "pager/src/scrollback/blocks/markdown_content.rs",
    "pager-render/src/prompt_images.rs", "chat-state/src/timeline.rs",
    "acp-transport/src/gateway.rs",
]
expected = json.loads((P2 / "source-after.json").read_text())
source_checks = {}
for suffix in hotspots:
    path = Path("crates/codegen") / suffix
    digest = hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
    source_checks[str(path)] = {"sha256": digest, "matches_p2": expected.get(str(path)) == digest}

summary = json.loads((P2 / "pty-log/attribution-summary.json").read_text())
output = {
    "kind": "reanalysis_of_retained_release_traces_not_new_measurements",
    "clock_caveat": "Within instrumentation clock only; batch/present proxies are not exact first history consumption/visibility.",
    "source_checks": source_checks,
    "after_2048_summary": [r for r in summary if r["stage"] == "after" and r["turns"] == 2048],
    "ordinary_2048_paint_proxies": proxies,
}
(HERE / "evidence.json").write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n")
assert all(row["matches_p2"] for row in source_checks.values()), "Hotspot source drift; revisit interpretation"
print("Retained traces: 3 ordinary paint proxies, 2 workload summaries; 9 hotspot sources match P2.")
