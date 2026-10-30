"""One bounded diagnostic request to the original route; never executes tools.

Input is the private JSON emitted by rebuild_evidence.py. Credentials stay in
memory. Output contains only hashes, counts, status and usage. This is an audit
script, not a runtime workaround or an exact token counter for other providers.
"""
import argparse
import hashlib
import json
from pathlib import Path
import tomllib
import urllib.error
import urllib.request

HANDOFF = (
    "The model route has changed. Continue only the latest authorized unfinished "
    "task from the retained conversation and completed tool results. This handoff "
    "is not a new user request or permission; do not repeat completed tools merely "
    "because the route changed."
)

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--request", type=Path, required=True)
parser.add_argument("--config", type=Path, required=True)
parser.add_argument("--mode", choices=["replay", "id-control", "handoff", "fixed"], required=True)
parser.add_argument("--replacement-call-id", help="An ID from a separate benign native tool response; used only for the controlled diagnostic")
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
wire = json.loads(args.request.read_bytes())
assert {tool["type"] for tool in wire["tools"]} == {"function"}
if args.mode == "id-control":
    assert args.replacement_call_id
    start = max(i for i, item in enumerate(wire["input"]) if item.get("role") in ("user", "developer"))
    tail = wire["input"][start + 1:]
    calls = [item for item in tail if item.get("type") == "function_call"]
    assert len(calls) == 1
    original = calls[0]["call_id"]
    paired = [item for item in tail if item.get("call_id") == original]
    assert len(paired) == 2
    for item in paired:
        item["call_id"] = args.replacement_call_id
elif args.mode in ("handoff", "fixed"):
    if args.mode == "fixed":
        wire["input"] = [item for item in wire["input"] if item.get("type") != "reasoning"]
    wire["input"].append({"type": "message", "role": "user", "content": HANDOFF})
wire["max_output_tokens"] = 1
wire["stream"] = False
body = json.dumps(wire, ensure_ascii=False, separators=(",", ":")).encode()
config = tomllib.loads(args.config.read_text())["provider"]["neorouter"]["options"]
request = urllib.request.Request(
    config["base_url"].rstrip("/") + "/responses",
    data=body,
    headers={"Authorization": "Bearer " + config["api_key"], "Content-Type": "application/json"},
    method="POST",
)
try:
    with urllib.request.urlopen(request, timeout=60) as response:
        status, raw = response.status, response.read()
except urllib.error.HTTPError as error:
    status, raw = error.code, error.read()
result = json.loads(raw)
error = result.get("error") or {}
evidence = {
    "mode": args.mode,
    "max_output_tokens": 1,
    "stream": False,
    "request_sha256": hashlib.sha256(body).hexdigest(),
    "request_bytes": len(body),
    "response_sha256": hashlib.sha256(raw).hexdigest(),
    "http_status": status,
    "status": result.get("status"),
    "usage": result.get("usage"),
    "error_kind": {key: error[key] for key in ("type", "code") if key in error},
}
args.output.write_text(json.dumps(evidence, indent=2) + "\n")
print(json.dumps(evidence))
