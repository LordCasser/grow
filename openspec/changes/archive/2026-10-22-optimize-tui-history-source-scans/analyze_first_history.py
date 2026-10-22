"""Correlate opt-in history checkpoints using their cursor and writer sequence."""
import datetime
import json
import re
import statistics
import sys
from pathlib import Path

directory = Path(sys.argv[1])


def fields(row):
    result = row.get("fields", {}).copy()
    debug = result.get("fields", "")
    for key, number in re.findall(r'"(\w+)": Number\((\d+)\)', debug):
        result[key] = int(number)
    for key, string in re.findall(r'"(\w+)": String\(("(?:\\.|[^"\\])*")\)', debug):
        result[key] = json.loads(string)
    return result


def timestamp_ms(row):
    return datetime.datetime.fromisoformat(row["timestamp"].replace("Z", "+00:00")).timestamp() * 1000


results = []
for path in sorted(directory.glob("*/*-loading.json")):
    document = json.loads(path.read_text())
    raw = [json.loads(line) for line in (path.parent / document["raw_files"]["instrumentation"]).read_text().splitlines()]
    named = lambda name: [r for r in raw if fields(r).get("name") == name]
    send, applied, frame = (named(name) for name in ("session.replay.first_send", "pager.replay.first_applied", "pager.replay.first_frame"))
    # The modern/slow root fixtures have one load and at least one visible row.
    assert len(send) == len(applied) == len(frame) == 1, (path, len(send), len(applied), len(frame))
    send, applied, frame = send[0], applied[0], frame[0]
    assert fields(send)["cursor"] == fields(applied)["cursor"] == fields(frame)["cursor"], path
    assert fields(send)["session_id"] == fields(applied)["session_id"] == fields(frame)["session_id"], path
    sequence = fields(frame)["writer_sequence"]
    acks = [r for r in raw if fields(r).get("event") == "frame_ack" and fields(r).get("sequence") == sequence]
    assert len(acks) == 1, (path, sequence, len(acks))
    deliveries = [r for r in raw if fields(r).get("name") == "pager.frame.delivery" and fields(r).get("sequence") == sequence]
    assert len(deliveries) == 1, (path, sequence)
    assert fields(deliveries[0]).get("success") is True, path
    presents = [r for r in raw if fields(r).get("name") == "pager.frame.present"
                and fields(r).get("queued_after") == sequence
                and fields(r).get("queued_before", sequence) < sequence]
    assert len(presents) == 1, (path, sequence, len(presents))
    loads = named("session.load_session")
    assert len(loads) == 1, path
    origin = timestamp_ms(loads[0]) - fields(loads[0])["elapsed_us"] / 1000
    # Stage times share the instrumentation UTC clock. Do not subtract PTY
    # spawn elapsed times or call first_frame the first physical source row.
    stage, case, repeat = path.parent.name.rsplit("-", 2)
    item = dict(stage=stage, case=case, repeat=int(repeat), turns=document["turns"], source=str(path.relative_to(directory)), cursor=fields(send)["cursor"], writer_sequence=sequence)
    for key, row in [("send_attempt", send), ("applied", applied), ("frame_produced", frame), ("frame_delivered", deliveries[0]), ("frame_ack_observed", acks[0])]:
        item[key + "_from_load_ms"] = timestamp_ms(row) - origin
    item["send_to_applied_ms"] = timestamp_ms(applied) - timestamp_ms(send)
    item["applied_to_frame_produced_ms"] = timestamp_ms(frame) - timestamp_ms(applied)
    item["first_present_work_ms"] = fields(presents[0])["elapsed_us"] / 1000
    present_start = timestamp_ms(presents[0]) - item["first_present_work_ms"]
    item["first_present_start_from_load_ms"] = present_start - origin
    item["applied_to_first_present_start_ms"] = present_start - timestamp_ms(applied)
    results.append(item)
(directory / "first-history.json").write_text(json.dumps(results, indent=2) + "\n")
summary = []
for stage, case, turns in sorted({(r["stage"], r["case"], r["turns"]) for r in results}):
    selected = [r for r in results if (r["stage"], r["case"], r["turns"]) == (stage, case, turns)]
    metrics = {}
    for key in selected[0]:
        if key.endswith("_ms"):
            values = [r[key] for r in selected]
            metrics[key] = dict(median_ms=statistics.median(values), range_ms=[min(values), max(values)])
    summary.append(dict(stage=stage, case=case, turns=turns, n=len(selected), metrics=metrics))
(directory / "first-history-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(f"Correlated {len(results)} successful root replay pipelines; all cursor/session/sequence checks passed.")
