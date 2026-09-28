#!/usr/bin/env python3
"""Offline, read-only prompt cache evidence comparison for Grow snapshots."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import stat
import sys
from typing import Any

# Timeline message events can contain accepted inline images even though the
# selected sampling request body has its own smaller transport limit.
MAX_LINE_BYTES = 128 * 1024 * 1024
MAX_BODY_BYTES = 64 * 1024 * 1024
CHUNK_BYTES = 64 * 1024
HEX = re.compile(r"^[0-9a-f]{64}$")

# Minimal BLAKE3 implementation used to validate evidence with no third-party
# runtime dependency. It follows the unkeyed hash mode from the BLAKE3 spec.
_IV = (0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
       0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19)
_PERM = (2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8)
_CHUNK_START, _CHUNK_END, _PARENT, _ROOT = 1, 2, 4, 8
_MASK = 0xffffffff


def _ror(x: int, n: int) -> int:
    return ((x >> n) | (x << (32 - n))) & _MASK


def _g(s: list[int], a: int, b: int, c: int, d: int, x: int, y: int) -> None:
    s[a] = (s[a] + s[b] + x) & _MASK
    s[d] = _ror(s[d] ^ s[a], 16)
    s[c] = (s[c] + s[d]) & _MASK
    s[b] = _ror(s[b] ^ s[c], 12)
    s[a] = (s[a] + s[b] + y) & _MASK
    s[d] = _ror(s[d] ^ s[a], 8)
    s[c] = (s[c] + s[d]) & _MASK
    s[b] = _ror(s[b] ^ s[c], 7)


def _compress(cv: tuple[int, ...], words: tuple[int, ...], counter: int,
              block_len: int, flags: int) -> tuple[int, ...]:
    s = list(cv) + list(_IV[:4]) + [counter & _MASK, counter >> 32, block_len, flags]
    m = list(words)
    for _ in range(7):
        _g(s, 0, 4, 8, 12, m[0], m[1]); _g(s, 1, 5, 9, 13, m[2], m[3])
        _g(s, 2, 6, 10, 14, m[4], m[5]); _g(s, 3, 7, 11, 15, m[6], m[7])
        _g(s, 0, 5, 10, 15, m[8], m[9]); _g(s, 1, 6, 11, 12, m[10], m[11])
        _g(s, 2, 7, 8, 13, m[12], m[13]); _g(s, 3, 4, 9, 14, m[14], m[15])
        m = [m[i] for i in _PERM]
    return tuple((s[i] ^ s[i + 8]) & _MASK for i in range(8)) + tuple(
        (s[i + 8] ^ cv[i]) & _MASK for i in range(8))


class _Output:
    def __init__(self, cv: tuple[int, ...], words: tuple[int, ...], counter: int,
                 block_len: int, flags: int):
        self.cv, self.words, self.counter = cv, words, counter
        self.block_len, self.flags = block_len, flags

    def chaining(self) -> tuple[int, ...]:
        return _compress(self.cv, self.words, self.counter, self.block_len, self.flags)[:8]

    def root_bytes(self, length: int = 32) -> bytes:
        result = bytearray()
        counter = 0
        while len(result) < length:
            words = _compress(self.cv, self.words, counter, self.block_len, self.flags | _ROOT)
            result.extend(b"".join(w.to_bytes(4, "little") for w in words))
            counter += 1
        return bytes(result[:length])


def _block_words(data: bytes) -> tuple[int, ...]:
    data = data.ljust(64, b"\0")
    return tuple(int.from_bytes(data[i:i + 4], "little") for i in range(0, 64, 4))


def _chunk_output(data: bytes, chunk_counter: int) -> _Output:
    cv = _IV
    blocks = [data[i:i + 64] for i in range(0, len(data), 64)] or [b""]
    for i, block in enumerate(blocks):
        flags = (_CHUNK_START if i == 0 else 0) | (_CHUNK_END if i == len(blocks) - 1 else 0)
        out = _Output(cv, _block_words(block), chunk_counter, len(block), flags)
        if i != len(blocks) - 1:
            cv = out.chaining()
    return out


def _parent_output(left: tuple[int, ...], right: tuple[int, ...]) -> _Output:
    return _Output(_IV, tuple(left + right), 0, 64, _PARENT)


def blake3(data: bytes) -> str:
    chunks = [data[i:i + 1024] for i in range(0, len(data), 1024)] or [b""]
    stack: list[tuple[int, ...]] = []
    for index, chunk in enumerate(chunks[:-1]):
        cv = _chunk_output(chunk, index).chaining()
        total = index + 1
        while total & 1 == 0:
            cv = _parent_output(stack.pop(), cv).chaining()
            total >>= 1
        stack.append(cv)
    output = _chunk_output(chunks[-1], len(chunks) - 1)
    while stack:
        output = _parent_output(stack.pop(), output.chaining())
    return output.root_bytes().hex()


class EvidenceError(Exception):
    pass


def _safe_regular(path: Path, root: Path, label: str) -> None:
    try:
        resolved = path.resolve(strict=True)
        resolved.relative_to(root.resolve(strict=True))
    except (OSError, ValueError) as exc:
        raise EvidenceError(f"{label} escapes snapshot or is missing: {path}") from exc
    if path.is_symlink() or not stat.S_ISREG(path.lstat().st_mode):
        raise EvidenceError(f"{label} must be a regular non-symlink file: {path}")


def _read_jsonl(path: Path, root: Path):
    _safe_regular(path, root, "timeline")
    with path.open("rb") as handle:
        number = 0
        while True:
            raw = handle.readline(MAX_LINE_BYTES + 1)
            if not raw:
                break
            number += 1
            if len(raw) > MAX_LINE_BYTES:
                raise EvidenceError(f"timeline line {number} exceeds {MAX_LINE_BYTES} bytes")
            try:
                yield json.loads(raw)
            except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                raise EvidenceError(f"invalid timeline JSON at line {number}: {exc}") from exc


def _observation(event: Any) -> dict[str, Any] | None:
    if not isinstance(event, dict) or event.get("type") != "observation":
        return None
    payload = event.get("event")
    if isinstance(payload, dict) and payload.get("scope") == "sampling_evidence":
        return payload
    return None


def _safe_chunk(root: Path, digest: str, expected: int) -> bytes:
    if not isinstance(digest, str) or not HEX.fullmatch(digest):
        raise EvidenceError("invalid sampling chunk digest reference")
    path = root / "artifacts" / "sampling" / f"{digest}.bin"
    # Check each path component: a symlinked directory can escape even when the
    # final file itself is regular.
    for component in (root / "artifacts", root / "artifacts" / "sampling"):
        if component.is_symlink():
            raise EvidenceError(f"sampling artifact path contains symlink: {component}")
    _safe_regular(path, root, "sampling chunk")
    if path.stat().st_size > CHUNK_BYTES:
        raise EvidenceError(f"sampling chunk exceeds {CHUNK_BYTES} bytes")
    data = path.read_bytes()
    if len(data) != expected or blake3(data) != digest:
        raise EvidenceError(f"sampling chunk content mismatch: {digest}")
    return data


def _load_record(root: Path, observation: dict[str, Any], seq: Any) -> dict[str, Any]:
    data = observation.get("data")
    if not isinstance(data, dict):
        raise EvidenceError(f"sampling observation {seq} has no record")
    size, chunks = data.get("bytes"), data.get("chunks")
    if not isinstance(size, int) or size < 0 or size > MAX_BODY_BYTES or not isinstance(chunks, list):
        raise EvidenceError(f"sampling observation {seq} has invalid body size/chunks")
    if len(chunks) != (size + CHUNK_BYTES - 1) // CHUNK_BYTES:
        raise EvidenceError(f"sampling observation {seq} has invalid chunk count")
    result = bytearray()
    for i, chunk in enumerate(chunks):
        expected = min(CHUNK_BYTES, size - i * CHUNK_BYTES)
        if not isinstance(chunk, dict) or chunk.get("bytes") != expected:
            raise EvidenceError(f"sampling observation {seq} has invalid chunk length")
        result.extend(_safe_chunk(root, chunk.get("blake3"), expected))
    if len(result) != size:
        raise EvidenceError(f"sampling observation {seq} body length mismatch")
    return {"owner": data.get("owner"), "kind": data.get("kind"),
            "metadata": data.get("metadata"), "bytes": bytes(result), "seq": seq}


def _identity(record: dict[str, Any]) -> str | None:
    owner, metadata = record.get("owner"), record.get("metadata")
    owner = owner if isinstance(owner, dict) else {}
    metadata = metadata if isinstance(metadata, dict) else {}
    sid = owner.get("sideband_id")
    attempt = metadata.get("sideband_attempt", metadata.get("attempt_number"))
    if isinstance(sid, str) and isinstance(attempt, int):
        return f"{sid}:{attempt}"
    rid = owner.get("request_id")
    if isinstance(rid, str) and isinstance(attempt, int):
        return f"{rid}:{attempt}"
    return None


def _collect(root: Path, timeline: Path) -> tuple[dict[str, dict[str, Any]], dict[str, Any], set[str]]:
    selected: dict[str, dict[str, Any]] = {}
    usage: dict[str, Any] = {}
    responses: set[str] = set()
    for event in _read_jsonl(timeline, root):
        if isinstance(event, dict) and event.get("type") == "request":
            payload = event.get("event", {})
            if isinstance(payload, dict) and payload.get("state") == "completed":
                rid, attempt = payload.get("id"), payload.get("attempt", 0)
                if isinstance(rid, str) and isinstance(attempt, int):
                    usage[f"{rid}:{attempt}"] = payload
        if isinstance(event, dict) and event.get("type") == "observation":
            payload = event.get("event")
            if isinstance(payload, dict) and payload.get("scope") == "sampling_usage" and payload.get("name") == "aux_attempt_settled":
                data = payload.get("data")
                key = data.get("key") if isinstance(data, dict) else None
                if isinstance(key, dict) and isinstance(key.get("sideband_id"), str) and isinstance(key.get("attempt_no"), int):
                    usage[f"{key['sideband_id']}:{key['attempt_no']}"] = {"usage": _sideband_usage(data.get("usage")), "source": "auxiliary_settlement"}
        observation = _observation(event)
        if observation is None or observation.get("name") not in ("request", "response"):
            continue
        data = observation.get("data")
        if not isinstance(data, dict):
            raise EvidenceError(f"sampling observation {event.get('seq')} has no record")
        rec = {"owner": data.get("owner"), "metadata": data.get("metadata"),
               "kind": data.get("kind"), "seq": event.get("seq")}
        identity = _identity(rec)
        if identity is None:
            continue
        # Validate / materialize the selected body only after selectors are
        # known. Keep metadata-only index bounded to a single timeline scan.
        if observation["name"] == "request":
            selected[identity] = rec
        else:
            responses.add(identity)
    return selected, usage, responses


def _sideband_usage(value: Any) -> dict[str, int] | None:
    if not isinstance(value, dict):
        return None
    result = {}
    for source, target in (("prompt_tokens", "input_tokens"), ("completion_tokens", "output_tokens")):
        if isinstance(value.get(source), int) and value[source] >= 0:
            result[target] = value[source]
    for bucket, amount in (("read", "cached_prompt_tokens"), ("write", "cache_creation_prompt_tokens")):
        if value.get(f"cache_{bucket}_known") is True and isinstance(value.get(amount), int) and value[amount] >= 0:
            result[f"cache_{bucket}_tokens"] = value[amount]
    return result


def _load_selected(root: Path, timeline: Path, identity: str, kind: str = "request") -> dict[str, Any]:
    found = None
    for event in _read_jsonl(timeline, root):
        observation = _observation(event)
        if observation is None or observation.get("name") != kind:
            continue
        data = observation.get("data")
        rec = {"owner": data.get("owner") if isinstance(data, dict) else None,
               "metadata": data.get("metadata") if isinstance(data, dict) else None,
               "kind": data.get("kind") if isinstance(data, dict) else None}
        if _identity(rec) == identity:
            found = _load_record(root, observation, event.get("seq"))
    if found is None:
        raise EvidenceError(f"{kind} identity not found: {identity}")
    return found


def _digest_json(value: Any) -> str:
    raw = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    return hashlib.sha256(raw).hexdigest()


def _first_diff(a: Any, b: Any, path: str = "$") -> str | None:
    if type(a) is not type(b):
        return path
    if isinstance(a, dict):
        for key in sorted(set(a) | set(b)):
            if key not in a or key not in b:
                return f"{path}.{key}"
            diff = _first_diff(a[key], b[key], f"{path}.{key}")
            if diff:
                return diff
        return None
    if isinstance(a, list):
        for i, (left, right) in enumerate(zip(a, b)):
            diff = _first_diff(left, right, f"{path}[{i}]")
            if diff:
                return diff
        return f"{path}[{min(len(a), len(b))}]" if len(a) != len(b) else None
    return None if a == b else path


def _protocol(body: Any) -> str:
    if not isinstance(body, dict):
        return "unknown"
    if "input" in body or "instructions" in body:
        return "responses"
    if "messages" in body and "max_tokens" in body:
        return "messages"
    if "messages" in body:
        return "chat_completions"
    return "unknown"


def _section(body: dict[str, Any], protocol: str, section: str) -> Any:
    keys = {
        "tools": ["tools"],
        "system": ["system", "instructions"],
        "history": ["messages"] if protocol in ("messages", "chat_completions") else ["input"],
        "settings": ["model", "temperature", "top_p", "max_tokens", "max_output_tokens", "reasoning", "thinking", "text", "response_format", "tool_choice", "parallel_tool_calls", "stream"],
        "cache_hints": ["prompt_cache_key", "prompt_cache_retention", "cache_control", "cache_creation", "cache_read_input_tokens"],
    }[section]
    if section == "cache_hints":
        found: dict[str, str] = {}
        wanted = {"prompt_cache_key", "prompt_cache_retention", "cache_control",
                  "cache_creation", "cache_read_input_tokens"}
        def visit(value: Any, path: str = "$") -> None:
            if isinstance(value, dict):
                for key, child in value.items():
                    child_path = f"{path}.{key}"
                    if key in wanted:
                        found[child_path] = _digest_json(child)
                    visit(child, child_path)
            elif isinstance(value, list):
                for index, child in enumerate(value):
                    visit(child, f"{path}[{index}]")
        visit(body)
        return found
    values = {key: body[key] for key in keys if key in body}
    return values


def _compare_body(base: dict[str, Any], request: dict[str, Any]) -> dict[str, Any]:
    bp, rp = _protocol(base), _protocol(request)
    sections = {}
    for name in ("tools", "system", "history", "settings", "cache_hints"):
        left, right = _section(base, bp, name), _section(request, rp, name)
        diff = _first_diff(left, right)
        item: dict[str, Any] = {"changed": diff is not None,
                                "baseline_digest": _digest_json(left),
                                "request_digest": _digest_json(right)}
        if diff:
            item["first_change_path"] = diff
        if name == "history":
            item["append_only"] = isinstance(left, dict) and isinstance(right, dict) and len(left) == len(right) == 1 and list(left) == list(right) and isinstance(next(iter(left.values())), list) and isinstance(next(iter(right.values())), list) and next(iter(right.values()))[:len(next(iter(left.values())))] == next(iter(left.values()))
        sections[name] = item
    return {"protocol": {"baseline": bp, "request": rp, "same": bp == rp}, "sections": sections}


def _usage(event: dict[str, Any] | None) -> dict[str, Any]:
    if not event:
        return {"status": "unavailable"}
    value = event.get("usage")
    if not isinstance(value, dict):
        return {"status": "unavailable"}
    # RequestEvent.usage is the durable normalized settlement, not the raw
    # provider response. Preserve both field presence and explicit zero.
    known = {key: value[key] for key in ("input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens") if isinstance(value.get(key), int) and value[key] >= 0}
    full, read, write = (value.get(key) for key in ("input_tokens", "cache_read_tokens", "cache_write_tokens"))
    valid_full = isinstance(full, int) and full >= 0
    valid_read = valid_full and isinstance(read, int) and 0 <= read <= full
    valid_write = valid_full and isinstance(write, int) and 0 <= write <= full
    if valid_read and valid_write and read + write > full:
        valid_read = valid_write = False
    return {"status": "known" if known else "unavailable", "source": event.get("source", "timeline_settlement"),
            "fields": known,
            "field_presence": {key: key in known for key in ("input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens")},
            "cache_coverage": {"read_known_input_tokens": full if valid_read else 0,
                               "read_unknown_calls": 0 if valid_read else 1,
                               "write_unknown_calls": 0 if valid_write else 1} if valid_full else {"status": "unavailable"}}


def _raw_usage(body: bytes) -> dict[str, Any]:
    """Report allowlisted numeric fields from captured provider frames only."""
    names = {"input_tokens", "output_tokens", "prompt_tokens", "completion_tokens",
             "total_tokens", "cache_read_input_tokens", "cache_creation_input_tokens",
             "prompt_cache_hit_tokens", "prompt_cache_miss_tokens", "cached_tokens",
             "cache_write_tokens", "reasoning_tokens"}
    frames: list[Any] = []
    try:
        frames.append(json.loads(body))
    except (UnicodeDecodeError, json.JSONDecodeError):
        for line in body.splitlines():
            if not line.startswith(b"data:"):
                continue
            try:
                frames.append(json.loads(line[5:].strip()))
            except (UnicodeDecodeError, json.JSONDecodeError):
                continue
    found: dict[str, dict[str, int]] = {}
    for frame in frames:
        if not isinstance(frame, dict):
            continue
        kind = frame.get("type")
        if kind not in ("message_start", "message_delta", "response.completed",
                        "response.incomplete", "response.failed", "chat.completion.chunk"):
            kind = "usage"
        container = frame.get("response") if isinstance(frame.get("response"), dict) else frame.get("message")
        source = container if isinstance(container, dict) else frame
        usage = source.get("usage") if isinstance(source, dict) else None
        if not isinstance(usage, dict):
            continue
        fields: dict[str, int] = {}
        for key, value in usage.items():
            if key in names and isinstance(value, int) and value >= 0:
                fields[key] = value
            elif key in ("input_tokens_details", "prompt_tokens_details", "output_tokens_details") and isinstance(value, dict):
                for nested, number in value.items():
                    if nested in names and isinstance(number, int) and number >= 0:
                        fields[f"{key}.{nested}"] = number
        if fields:
            found[kind] = fields
    return {"status": "known" if found else "unavailable", "source": "provider_response_evidence",
            "frames": found}


def _cache_cross_check(raw: dict[str, Any], settled: dict[str, Any]) -> dict[str, str]:
    frames = raw.get("frames") if isinstance(raw.get("frames"), dict) else {}
    fields = settled.get("fields") if isinstance(settled.get("fields"), dict) else {}
    aliases = {
        "read": ("input_tokens_details.cached_tokens", "prompt_tokens_details.cached_tokens",
                 "cache_read_input_tokens", "prompt_cache_hit_tokens"),
        "write": ("input_tokens_details.cache_write_tokens", "cache_creation_input_tokens"),
    }
    result = {}
    for bucket, names in aliases.items():
        raw_value = next((frame[name] for frame in reversed(list(frames.values()))
                          for name in names if name in frame), None)
        settled_value = fields.get(f"cache_{bucket}_tokens")
        result[bucket] = ("unavailable" if raw_value is None or settled_value is None
                          else "matches" if raw_value == settled_value else "differs")
    return result


def _report(root: Path, baseline_id: str, request_id: str) -> dict[str, Any]:
    timeline = root / "timeline.jsonl"
    index, usage, responses = _collect(root, timeline)
    if baseline_id not in index or request_id not in index:
        missing = baseline_id if baseline_id not in index else request_id
        raise EvidenceError(f"request identity not found: {missing}")
    baseline = _load_selected(root, timeline, baseline_id)
    request = _load_selected(root, timeline, request_id)
    raw_baseline = _raw_usage(_load_selected(root, timeline, baseline_id, "response")["bytes"]) if baseline_id in responses else {"status": "unavailable"}
    raw_request = _raw_usage(_load_selected(root, timeline, request_id, "response")["bytes"]) if request_id in responses else {"status": "unavailable"}
    settled_baseline, settled_request = _usage(usage.get(baseline_id)), _usage(usage.get(request_id))
    for item in (baseline, request):
        try:
            item["json"] = json.loads(item["bytes"])
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise EvidenceError(f"request body at timeline seq {item['seq']} is not valid JSON") from exc
    bm = baseline["metadata"] if isinstance(baseline["metadata"], dict) else {}
    rm = request["metadata"] if isinstance(request["metadata"], dict) else {}
    bo = baseline["owner"] if isinstance(baseline["owner"], dict) else {}
    ro = request["owner"] if isinstance(request["owner"], dict) else {}
    same_scope = bo.get("sideband_id") == ro.get("sideband_id")
    bp, rp = bo.get("source_projection"), ro.get("source_projection")
    same_branch = (isinstance(bp, dict) and isinstance(rp, dict)
                   and isinstance(bp.get("timeline_id"), str)
                   and bp.get("timeline_id") == rp.get("timeline_id"))
    source_ordered = (same_branch and isinstance(bp.get("source_seq"), int)
                      and isinstance(rp.get("source_seq"), int)
                      and bp["source_seq"] < rp["source_seq"])
    same_visible_route = bool(bm.get("route") and rm.get("route") and bm.get("route") == rm.get("route") and bm.get("backend") == rm.get("backend"))
    # Evidence stores a redacted endpoint, not a deployment, tenant, or
    # credential identity. Equality of that endpoint cannot confirm a route.
    comparability = "not_comparable"
    return {
        "schema_version": 1,
        "baseline": {"identity": baseline_id, "seq": baseline["seq"], "bytes": len(baseline["bytes"]), "digest_blake3": blake3(baseline["bytes"]), "backend": bm.get("backend", "unavailable"), "route": {"status": "known" if bm.get("route") else "unavailable", "digest": _digest_json(bm["route"]) if bm.get("route") else None}, "usage": settled_baseline, "raw_usage": raw_baseline, "cache_cross_check": _cache_cross_check(raw_baseline, settled_baseline)},
        "request": {"identity": request_id, "seq": request["seq"], "bytes": len(request["bytes"]), "digest_blake3": blake3(request["bytes"]), "backend": rm.get("backend", "unavailable"), "route": {"status": "known" if rm.get("route") else "unavailable", "digest": _digest_json(rm["route"]) if rm.get("route") else None}, "usage": settled_request, "raw_usage": raw_request, "cache_cross_check": _cache_cross_check(raw_request, settled_request)},
        "comparability": {"status": comparability, "same_sideband_scope": same_scope, "same_branch": same_branch, "source_ordered": source_ordered, "same_visible_endpoint": same_visible_route, "same_confirmed_route": False, "reason": "snapshot has no durable tenant/deployment identity; explicit source projection, source order, same scope, and confirmed route are required"},
        "comparison": _compare_body(baseline["json"], request["json"]),
        "source_projection": _projection_diff(bp, rp),
        "limits": ["Visible request JSON and local evidence only; no tokenizer or provider cache state is available.", "Stable visible fields do not prove a cache hit; differences do not establish a provider miss cause."],
    }


def _projection_diff(base: Any, request: Any) -> dict[str, Any]:
    left = base if isinstance(base, dict) else {}
    right = request if isinstance(request, dict) else {}
    result: dict[str, Any] = {
        "baseline_digest": _digest_json(base) if base is not None else None,
        "request_digest": _digest_json(request) if request is not None else None,
        "changed": base != request,
    }
    for key in ("surface_ids", "image_budget", "continuation_epoch", "portable_prefix_len", "portable_reasoning_backend", "native_spans"):
        result[key] = {"changed": left.get(key) != right.get(key),
                       "baseline_digest": _digest_json(left[key]) if key in left else None,
                       "request_digest": _digest_json(right[key]) if key in right else None}
    return result


def _text(report: dict[str, Any]) -> str:
    lines = [f"Prompt cache evidence comparison: {report['baseline']['identity']} -> {report['request']['identity']}",
             f"Comparable: {report['comparability']['status']}",
             f"Protocol: {report['comparison']['protocol']['baseline']} -> {report['comparison']['protocol']['request']}",
             f"Body bytes: {report['baseline']['bytes']} -> {report['request']['bytes']}",
             f"Body BLAKE3: {report['baseline']['digest_blake3']} -> {report['request']['digest_blake3']}"]
    for name, item in report["comparison"]["sections"].items():
        lines.append(f"{name}: {'changed at ' + item['first_change_path'] if item['changed'] else 'unchanged'}")
    lines.append(f"Usage: {report['baseline']['usage']['status']} -> {report['request']['usage']['status']}")
    lines.extend(report["limits"])
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--session-dir", required=True, type=Path, help="completed session snapshot directory")
    parser.add_argument("--request", required=True, help="request identity ID:ATTEMPT")
    parser.add_argument("--baseline", required=True, help="baseline identity ID:ATTEMPT")
    parser.add_argument("--format", choices=("json", "text"), default="text")
    args = parser.parse_args(argv)
    try:
        root = args.session_dir
        if root.is_symlink() or not root.is_dir():
            raise EvidenceError("session snapshot must be a directory, not a symlink")
        report = _report(root, args.baseline, args.request)
        print(json.dumps(report, ensure_ascii=False, sort_keys=True, indent=2) if args.format == "json" else _text(report))
        return 0
    except EvidenceError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
