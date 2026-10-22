#!/usr/bin/env python3
"""Summarize resume PTY measurements from a results.json directory."""

from __future__ import annotations

import json
import re
import statistics
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any


LINE_RE = re.compile(r"^\[(resume-pty|resume-progress)\]\s*(.*)$")
FIELD_RE = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)=([^\s]+)")
NUMBER_RE = re.compile(r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?\Z")
OPTIONAL_NUMBER_RE = re.compile(
    r"Some\(([-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?)\)\Z"
)


def numeric_value(token: str) -> tuple[float | None, bool]:
    """Return (number, unavailable); booleans and null-like tokens are ignored."""
    if token == "unavailable":
        return None, True
    if token in {"true", "false", "None", "null"}:
        return None, False
    if NUMBER_RE.fullmatch(token):
        return float(token), False
    optional = OPTIONAL_NUMBER_RE.fullmatch(token)
    if optional:
        return float(optional.group(1)), False
    return None, False


def parse_measurement_lines(row: dict[str, Any]) -> list[dict[str, Any]]:
    line = row.get("line")
    if not isinstance(line, str):
        return []
    match = LINE_RE.match(line.strip())
    if not match:
        return []
    metric_type, fields = match.groups()
    parsed: list[dict[str, Any]] = []
    for key, token in FIELD_RE.findall(fields):
        if key == "turns":
            continue
        value, unavailable = numeric_value(token)
        if value is None and not unavailable:
            continue
        parsed.append(
            {
                "metric": key,
                "value": value,
                "unavailable": unavailable,
                "metric_type": metric_type,
            }
        )
    return parsed


def summarize(directory: Path) -> list[dict[str, Any]]:
    input_path = directory / "results.json"
    rows = json.loads(input_path.read_text(encoding="utf-8"))
    if not isinstance(rows, list):
        raise ValueError(f"{input_path} must contain a JSON array")

    groups: dict[tuple[str, str, int, str], dict[str, list[dict[str, Any]]]] = defaultdict(
        lambda: defaultdict(list)
    )
    for row in rows:
        if not isinstance(row, dict):
            continue
        line = row.get("line")
        if not isinstance(line, str):
            continue
        match = LINE_RE.match(line.strip())
        if not match:
            continue
        metric_type = match.group(1)
        fields = dict(FIELD_RE.findall(match.group(2)))
        raw_turns = fields.get("turns")
        if not raw_turns or not NUMBER_RE.fullmatch(raw_turns):
            continue
        turns_float = float(raw_turns)
        if not turns_float.is_integer():
            continue
        key = (
            str(row.get("stage", "unknown")),
            str(row.get("case", "unknown")),
            int(turns_float),
            metric_type,
        )
        for measurement in parse_measurement_lines(row):
            groups[key][measurement["metric"]].append(
                {
                    "repeat": row.get("repeat"),
                    "source": row.get("source"),
                    "value": measurement["value"],
                    "unavailable": measurement["unavailable"],
                    "line": line,
                }
            )

    summary: list[dict[str, Any]] = []
    for (stage, case, turns, metric_type), metrics in sorted(groups.items()):
        metric_rows: dict[str, Any] = {}
        for metric, observations in sorted(metrics.items()):
            numeric = [item["value"] for item in observations if item["value"] is not None]
            unavailable_runs = sum(item["unavailable"] for item in observations)
            metric_rows[metric] = {
                "median": statistics.median(numeric) if numeric else None,
                "range": [min(numeric), max(numeric)] if numeric else None,
                "runs": len(numeric),
                "unavailable_runs": unavailable_runs,
                "observations": observations,
            }
        summary.append(
            {
                "stage": stage,
                "case": case,
                "turns": turns,
                "metric_type": metric_type,
                "metrics": metric_rows,
            }
        )

    (directory / "metrics-summary.json").write_text(
        json.dumps(summary, indent=2, ensure_ascii=False, allow_nan=False) + "\n",
        encoding="utf-8",
    )
    return summary


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {Path(sys.argv[0]).name} DIRECTORY", file=sys.stderr)
        return 2
    directory = Path(sys.argv[1])
    try:
        summary = summarize(directory)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"wrote {directory / 'metrics-summary.json'} ({len(summary)} groups)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
