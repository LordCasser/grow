#!/usr/bin/env python3
"""Extract shareable counts and fixture results from this validation's private run.

Provider bodies, reasoning text, credentials, and complete prompts stay private.
The raw ACP stream also permits recovery if the driver's final cleanup failed.
"""
import argparse
import collections
import hashlib
import json
import re
from pathlib import Path
from urllib.parse import urlsplit


def digest(data):
    return hashlib.sha256(data).hexdigest()


def error_facts(error):
    if not isinstance(error, dict):
        return None
    data = error.get("data", error)
    return {"code": error.get("code"), "kind": data.get("error_kind", data.get("kind")),
            "status": data.get("http_status", data.get("status_code")),
            "portable_reasoning_required": data.get("portable_reasoning_required"),
            "sha256": digest(json.dumps(error, sort_keys=True).encode())}


def observations(value):
    if isinstance(value, dict):
        if value.get("scope") == "sampling_evidence":
            yield value
        else:
            for child in value.values():
                yield from observations(child)
    elif isinstance(value, list):
        for child in value:
            yield from observations(child)


def texts(value):
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "\n".join(texts(v) for v in value)
    if isinstance(value, dict):
        return texts(value.get("text", value.get("content", "")))
    return ""


def wire_stats(wire, backend):
    items = wire.get("input", wire.get("messages", []))
    users = [texts(v.get("content")) for v in items if v.get("role") == "user"]
    prompt = next((v for v in reversed(users) if "For this validation stage" in v), "")
    match = re.search(r"record_step.py (\d+)", prompt)
    stage = int(match[1]) if match else ("history_second" if any("multiply it by 3" in v for v in users) else "history_first")
    calls, results, thoughts = [], [], []
    opaque_count = 0
    assistant_count = 0
    for item in items:
        if backend == "chat_completions":
            if item.get("role") == "assistant":
                assistant_count += 1
                calls.extend(v["id"] for v in item.get("tool_calls", []))
                if "reasoning_content" in item:
                    thoughts.append(item["reasoning_content"] or "")
            if item.get("role") == "tool":
                results.append(item["tool_call_id"])
        elif backend == "responses":
            if item.get("role") == "assistant":
                assistant_count += 1
            if item.get("type") == "function_call":
                calls.append(item["call_id"])
            if item.get("type") == "function_call_output":
                results.append(item["call_id"])
            if item.get("type") == "reasoning":
                thoughts.extend(v.get("text", "") for v in item.get("content", []) if v.get("type") == "reasoning_text")
                opaque_count += sum(bool(item.get(k)) for k in ("id", "encrypted_content", "status"))
        else:
            if item.get("role") == "assistant":
                assistant_count += 1
            for part in item.get("content", []) if isinstance(item.get("content"), list) else []:
                if part.get("type") == "tool_use":
                    calls.append(part["id"])
                if part.get("type") == "tool_result":
                    results.append(part["tool_use_id"])
                if part.get("type") == "thinking":
                    thoughts.append(part.get("thinking", ""))
                    opaque_count += bool(part.get("signature"))
    return {
        "stage": stage, "wire_model": wire.get("model"),
        "reasoning_effort": wire.get("reasoning_effort", wire.get("reasoning")),
        "thinking": wire.get("thinking"), "assistant_count": assistant_count,
        "reasoning_text_count": len(thoughts), "reasoning_nonempty_count": sum(bool(v) for v in thoughts),
        "reasoning_text_chars": sum(len(v) for v in thoughts),
        "reasoning_opaque_field_count": opaque_count,
        "tool_call_count": len(calls), "tool_result_count": len(results),
        "tool_ids_paired_once": len(calls) == len(set(calls)) and sorted(calls) == sorted(results),
        "tool_ids_sha256": digest(json.dumps(sorted(calls)).encode()),
    }


def extract(root):
    rpc_path = root / "rpc-raw.jsonl"
    rpc = [json.loads(line) for line in rpc_path.read_text().splitlines()]
    responses = {v["id"]: v for v in rpc if "id" in v and "method" not in v}
    records = [json.loads(v) for v in (root / "workspace/executions.jsonl").read_text().splitlines()]
    tokens = {v["stage"]: v["token"] for v in records}
    routes = {}
    prompts = []
    last_token = None
    for i, value in enumerate(rpc):
        client = value.get("client", {})
        params = client.get("params", {})
        result = responses.get(client.get("id"), {})
        method = client.get("method")
        if method == "session/new" and "result" in result:
            routes[result["result"]["sessionId"]] = params["_meta"]["modelId"]
        elif method == "session/set_config_option" and "result" in result:
            routes[params["sessionId"]] = params["value"]
        elif method == "session/prompt":
            sid = params["sessionId"]
            end = next((j for j in range(i + 1, len(rpc)) if rpc[j].get("id") == client["id"] and "method" not in rpc[j]), len(rpc))
            updates = [v["params"]["update"] for v in rpc[i + 1:end] if v.get("method") == "session/update" and v.get("params", {}).get("sessionId") == sid]
            answer = "".join(texts(v.get("content")) for v in updates if v.get("sessionUpdate") == "agent_message_chunk")
            calls = [v for v in updates if v.get("sessionUpdate") == "tool_call"]
            commands = [v.get("rawInput", {}).get("command") for v in calls]
            tool_updates = {v["toolCallId"]: [u for u in updates if u.get("toolCallId") == v["toolCallId"]] for v in calls}
            completed_ids = {key for key, values in tool_updates.items() if next((u["status"] for u in reversed(values) if "status" in u), None) == "completed"}
            prompt = texts(params["prompt"])
            match = re.search(r"record_step.py (\d+)", prompt)
            stage = int(match[1]) if match else ("history_second" if "multiply it by 3" in prompt else "history_first")
            checks = {"rpc_success": "result" in result}
            if match:
                checks.update({"single_expected_command": commands == [f"python3 record_step.py {stage}"],
                               "all_tool_calls_completed": len(completed_ids) == len(calls),
                               "current_token_in_completed_tool_result": any(tokens.get(stage, "") and tokens[stage] in json.dumps(tool_updates[key]) for key in completed_ids),
                               "reported_current_token": bool(tokens.get(stage) and tokens[stage] in answer),
                               "reported_previous_token": last_token is None or last_token in answer})
                for command in commands:
                    completed = re.fullmatch(r"python3 record_step.py (\d+)", command or "")
                    if completed and int(completed[1]) in tokens:
                        last_token = tokens[int(completed[1])]
            else:
                checks.update({"no_tools": not calls, "correct_answer": answer.strip() == ("6056" if stage == "history_first" else "18175")})
            prompts.append({"stage": stage, "session_id": sid, "route": routes[sid], "status": "passed" if all(checks.values()) else "failed",
                            "checks": checks, "tool_commands": [v if re.fullmatch(r"python3 record_step.py [1-9]", v or "") else {"sha256": digest(str(v).encode())} for v in commands],
                            "assistant_text_sha256": digest(answer.encode()),
                            "rpc_error": error_facts(result.get("error")), "stop_reason": result.get("result", {}).get("stopReason")})
    sessions = []
    for path in sorted(root.rglob("timeline.jsonl")):
        if "sidebands" in path.parts:
            continue
        attempts = []
        request_by_id = {}
        for line in path.read_text().splitlines():
            row = json.loads(line)
            for obs in observations(row):
                data = obs["data"]
                owner = data["owner"]
                if "request_id" not in owner:
                    continue
                metadata = data["metadata"]
                request_id = owner["request_id"]
                body = b"".join((path.parent / "artifacts/sampling" / (v["blake3"] + ".bin")).read_bytes() for v in data.get("chunks", []))
                event = {"timeline_seq": row["seq"], "kind": obs["name"], "request_id": request_id, "attempt_number": metadata.get("attempt_number"), "body_sha256": digest(body), "body_bytes": len(body)}
                if obs["name"] == "request":
                    projection = owner["source_projection"]
                    route = urlsplit(metadata["route"])
                    event.update(wire_stats(json.loads(body), metadata["backend"]))
                    event.update({"backend": metadata["backend"], "endpoint_host": route.hostname, "endpoint_path_sha256": digest(route.path.encode()),
                                  "native_span_count": len(projection["native_spans"]), "continuation_epoch": projection["continuation_epoch"],
                                  "portable_reasoning_backend": projection["portable_reasoning_backend"]})
                    request_by_id[request_id] = event
                else:
                    event["stage"] = request_by_id[request_id]["stage"]
                    event["status"] = metadata.get("status")
                    event["provider_terminal"] = metadata.get("provider_terminal")
                    outcome = metadata.get("outcome")
                    if outcome:
                        event["outcome"] = {k: v for k, v in outcome.items() if k != "details"}
                        event["outcome"]["error"] = error_facts(outcome.get("details", {}).get("error"))
                    if "error" in metadata:
                        event["error"] = error_facts(metadata["error"])
                    event["truncated"] = metadata.get("truncated_by_evidence_limit")
                attempts.append(event)
        sessions.append({"session_id": path.parent.name, "timeline_sha256": digest(path.read_bytes()), "attempts": attempts})
    return {"raw_rpc_sha256": digest(rpc_path.read_bytes()), "prompts": prompts, "sessions": sessions,
            "fixture_execution_counts": dict(collections.Counter(str(v["stage"]) for v in records)),
            "all_fixture_stages_executed_once": collections.Counter(v["stage"] for v in records) == {v: 1 for v in range(1, 10)},
            "limitations": ["Reasoning text and raw provider responses are omitted; hashes identify private evidence.",
                            "A failed stage may be executed by a later model from retained user history; per-prompt commands remain authoritative."]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--private-run", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = extract(args.private_run)
    with args.output.open("x") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    print(json.dumps({"prompts": len(result["prompts"]), "passed": sum(v["status"] == "passed" for v in result["prompts"]), "sessions": len(result["sessions"])}))
