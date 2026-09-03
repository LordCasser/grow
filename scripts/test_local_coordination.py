#!/usr/bin/env python3
"""Hermetic real-process coordination regression (stdlib only).

Run: python3 scripts/test_local_coordination.py --binary target/debug/grow
Uses isolated GROW_HOME, two independent stdio processes, a loopback model,
and the platform's unchanged default temp directory. Never calls a paid model.
"""

import argparse
import concurrent.futures
import http.server
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time
import uuid


def inquiry_id():
    # UUIDv7 without requiring Python 3.14.
    return str(uuid.UUID(int=(int(time.time() * 1000) << 80) | (7 << 76)
                         | (int.from_bytes(os.urandom(2), "big") & 0xFFF) << 64
                         | (2 << 62) | (int.from_bytes(os.urandom(8), "big") & ((1 << 62) - 1))))


def eventually(fn, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = fn()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError("condition did not become true before deadline")


class Model(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), ModelHandler)
        self.requests = []
        self.errors = []
        self.foreground = threading.Event()
        self.release = threading.Event()
        self.foreground_release = threading.Event()
        self.block = False
        self.tool_target = None
        self.lock = threading.Lock()
        threading.Thread(target=self.serve_forever, daemon=True).start()

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server_port}"

    def inquiries(self):
        with self.lock:
            return [r for r in self.requests if r[0]]

    def close(self):
        self.release.set()
        self.foreground_release.set()
        self.shutdown()
        self.server_close()


class ModelHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        self.send_response(200 if self.path.endswith("/models") else 404)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"object":"list","data":[]}')

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if not self.path.endswith(("/chat/completions", "/responses")):
            self.send_response(404)
            self.end_headers()
            return
        serialized = json.dumps(body)
        sideband = "Another local Grow session is asking" in serialized
        tool_flow = not sideband and "COORDINATION_TOOL_FLOW" in serialized
        with self.server.lock:
            self.server.requests.append((sideband, body))
            if sideband and body.get("tools"):
                self.server.errors.append("sideband unexpectedly advertised tools")
        if sideband:
            if self.server.block:
                self.server.release.wait(60)
        elif not tool_flow:
            self.server.foreground.set()
            self.server.foreground_release.wait(60)
        tool_call = None
        if tool_flow:
            if "call_coord_list_ui" not in serialized:
                tool_call = ("call_coord_list_ui", "list_active_sessions", {})
            elif "call_coord_ask_ui" not in serialized:
                tool_call = ("call_coord_ask_ui", "ask_session", {
                    "target_session_id": self.server.tool_target, "question": "UI tool flow status?"})
        answer = "COORDINATION_SIDE_ANSWER" if sideband else "TOOL_FLOW_DONE" if tool_flow else "FOREGROUND_ANSWER"
        model = body.get("model", "test-model")
        if self.path.endswith("/responses"):
            response = {"id": "resp_test", "object": "response", "created_at": 1234567890,
                        "model": model, "status": "completed", "output": [{"type": "message",
                        "id": "msg_test", "role": "assistant", "status": "completed",
                        "content": [{"type": "output_text", "text": answer, "annotations": []}]}],
                        "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}}
            events = [{"type": "response.created", "sequence_number": 0,
                       "response": dict(response, output=[], status="in_progress")},
                      {"type": "response.output_text.delta", "sequence_number": 1,
                       "item_id": "msg_test", "output_index": 0, "content_index": 0, "delta": answer},
                      {"type": "response.completed", "sequence_number": 2, "response": response}]
            if tool_call:
                call_id, name, arguments = tool_call
                item = {"type": "function_call", "id": "fc_" + call_id, "call_id": call_id,
                        "name": name, "arguments": json.dumps(arguments), "status": "completed"}
                response["output"] = [item]
                events = [events[0],
                          {"type": "response.output_item.added", "sequence_number": 1,
                           "output_index": 0, "item": dict(item, arguments="", status="in_progress")},
                          {"type": "response.function_call_arguments.delta", "sequence_number": 2,
                           "item_id": item["id"], "output_index": 0, "delta": item["arguments"]},
                          {"type": "response.output_item.done", "sequence_number": 3,
                           "output_index": 0, "item": item},
                          {"type": "response.completed", "sequence_number": 4, "response": response}]
        else:
            events = [{"id": "chatcmpl-test", "object": "chat.completion.chunk",
                       "created": 1234567890, "model": model, "choices": [{"index": 0,
                       "delta": {"role": "assistant", "content": answer}, "finish_reason": "stop"}]},
                      {"id": "chatcmpl-test", "object": "chat.completion.chunk", "choices": [],
                       "created": 1234567890, "model": model,
                       "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}]
            if tool_call:
                call_id, name, arguments = tool_call
                events[0]["choices"] = [{"index": 0, "delta": {"role": "assistant", "tool_calls": [{
                    "index": 0, "id": call_id, "type": "function",
                    "function": {"name": name, "arguments": json.dumps(arguments)}}]},
                    "finish_reason": "tool_calls"}]
        try:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            for event in events:
                self.wfile.write(f"data: {json.dumps(event)}\n\n".encode())
            self.wfile.write(b"data: [DONE]\n\n")
        except (BrokenPipeError, ConnectionResetError):
            pass  # Expected when cancelling a blocked inference request.


class Client:
    def __init__(self, binary, cwd, env, log):
        self.stderr = log.open("w")
        self.proc = subprocess.Popen([str(binary), "agent", "--no-leader", "stdio"],
                                     cwd=cwd, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=self.stderr, text=True, encoding="utf-8", bufsize=1)
        self.pending = {}
        self.lock = threading.Lock()
        self.counter = 0
        self.notices = []
        self.permissions = []
        self.approve = False
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def write(self, message):
        with self.lock:
            self.proc.stdin.write(json.dumps(message) + "\n")
            self.proc.stdin.flush()

    def read(self):
        for line in self.proc.stdout:
            try:
                value = json.loads(line)
            except ValueError:
                continue
            messages = value if isinstance(value, list) else [value]
            for message in messages:
                if "method" in message:
                    if "id" not in message:
                        self.notices.append(message)
                    elif message["method"] == "session/request_permission":
                        self.permissions.append(message)
                        options = message["params"]["options"]
                        desired = "allow_once" if self.approve else "reject_once"
                        option = next(x for x in options if x["kind"] == desired)
                        self.write({"jsonrpc": "2.0", "id": message["id"], "result": {
                            "outcome": {"outcome": "selected", "optionId": option["optionId"]}}})
                    else:
                        self.write({"jsonrpc": "2.0", "id": message["id"], "error": {
                            "code": -32601, "message": "unsupported test-client request"}})
                elif message.get("id") in self.pending:
                    self.pending[message["id"]].put(message)
        for result in list(self.pending.values()):
            result.put({"error": {"message": "Grow process closed stdout"}})

    def call(self, method, params, timeout=40, raw=False):
        with self.lock:
            self.counter += 1
            request_id = self.counter
            result = queue.Queue()
            self.pending[request_id] = result
        try:
            self.write({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
            reply = result.get(timeout=timeout)
            if raw:
                return reply
            assert "error" not in reply, (method, reply)
            return reply["result"]
        finally:
            self.pending.pop(request_id, None)

    def init(self):
        result = self.call("initialize", {"protocolVersion": 1, "clientCapabilities": {},
                           "_meta": {"clientType": "test-client", "startupHints": {
                               "nonInteractive": True, "skipGitStatus": True, "skipProjectLayout": True}}})
        capability = result["agentCapabilities"]["_meta"]["grow/coordination"]
        assert capability["version"] == 2 and "get" in capability["operations"], result
        self.call("authenticate", {"methodId": "provider.api_key", "_meta": {"headless": True}})

    def new(self, cwd):
        return self.call("session/new", {"cwd": str(cwd), "mcpServers": []})["sessionId"]

    def close(self):
        if self.proc.poll() is None:
            self.proc.stdin.close()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=5)
        self.reader.join(timeout=2)
        self.proc.stdout.close()
        self.stderr.close()


def run(binary, root):
    model = Model()
    clients = []
    pool = concurrent.futures.ThreadPoolExecutor(max_workers=6)
    try:
        home = root / "grow-home"
        home.mkdir()
        cwd = root / "workspace"
        cwd.mkdir()
        other = root / "other-workspace"
        other.mkdir()
        for directory in (cwd, other):
            subprocess.run(["git", "init", "-q", str(directory)], check=True)
        (home / "config.toml").write_text(f'''[models]
default = "mock/test-model"
[provider.mock.options]
base_url = "{model.url}"
[provider.mock.models.test-model]
context_window = 200000
''')
        env = dict(os.environ, GROW_HOME=str(home), GROW_API_KEY="coordination-test-key",
                   GROW_DISABLE_AUTO_UPDATE="1", NO_PROXY="127.0.0.1,localhost")
        for name in ("GROW_CLI_CHAT_PROXY_BASE_URL", "GROW_INFERENCE_BASE_URL", "GROW_MODELS_BASE_URL",
                     "GROW_FEEDBACK_BASE_URL", "GROW_CONVERSATIONS_BASE_URL"):
            env[name] = model.url
        def spawn(name):
            client = Client(binary, cwd, env, root / f"{name}.stderr.log")
            clients.append(client)
            client.init()
            return client
        a, b = spawn("a"), spawn("b")
        sa, sb = a.new(cwd), b.new(cwd)
        def coord(client, operation, **params):
            return client.call("_grow/coordination/" + operation, params)
        visible = eventually(lambda: coord(a, "list", sourceSessionId=sa)["sessions"])
        assert [s["sessionId"] for s in visible] == [sb], visible
        print("PASS discovery across independent processes with default TMPDIR", flush=True)

        foreground = pool.submit(b.call, "session/prompt", {"sessionId": sb,
                                 "prompt": [{"type": "text", "text": "FOREGROUND_BLOCK"}]})
        assert model.foreground.wait(20), "foreground did not reach local model"
        model.block = True
        qid = inquiry_id()
        ask = dict(inquiryId=qid, sourceSessionId=sa, targetSessionId=sb, question="What are you doing?")
        first = pool.submit(coord, a, "ask", **ask)
        eventually(lambda: len(model.inquiries()) == 1)
        state = coord(a, "get", inquiryId=qid, sourceSessionId=sa)
        assert state["phase"] == "running" and "outcome" not in state, state
        retry = pool.submit(coord, a, "ask", **ask)
        queued_id = inquiry_id()
        queued = pool.submit(coord, a, "ask", **dict(ask, inquiryId=queued_id, question="Next inquiry"))
        def is_queued():
            reply = a.call("_grow/coordination/get", {"inquiryId": queued_id, "sourceSessionId": sa}, raw=True)
            if "error" in reply:
                assert reply["error"]["data"]["code"] == "not_found", reply
                return False  # The concurrent ask may not have reached admission yet.
            return reply["result"]["phase"] == "queued"
        eventually(is_queued)
        assert len(model.inquiries()) == 1
        model.release.set()
        result = first.result(20)
        assert result["status"] == "answered" and result["answer"] == "COORDINATION_SIDE_ANSWER", result
        assert retry.result(20) == result
        assert queued.result(20)["status"] == "answered"
        assert len(model.inquiries()) == 2, "retry ran another model request"
        assert not foreground.done(), "target foreground unexpectedly completed"
        model.foreground_release.set()
        assert foreground.result(20)["stopReason"] == "end_turn"
        assert not model.errors, model.errors
        print("PASS busy foreground, running query, FIFO queue, same-ID dedup, tool-free sideband", flush=True)

        conflict = coord(a, "ask", **dict(ask, question="different payload"))
        assert conflict["error"]["code"] == "conflict", conflict
        missing_id = inquiry_id()
        missing = coord(a, "ask", **dict(ask, inquiryId=missing_id, targetSessionId="absent"))
        assert missing["error"]["code"] == "not_found", missing
        assert coord(a, "get", inquiryId=missing_id, sourceSessionId=sa)["outcome"] == missing
        denied = a.call("_grow/coordination/get", {"inquiryId": qid, "sourceSessionId": sb}, raw=True)
        assert denied["error"]["data"]["code"] == "permission_denied", denied
        print("PASS structured failures, payload conflicts, query source isolation", flush=True)

        model.release.clear()
        cancel_id = inquiry_id()
        cancelled = pool.submit(coord, a, "ask", **dict(ask, inquiryId=cancel_id, question="Cancel this"))
        eventually(lambda: len(model.inquiries()) == 3)
        assert coord(a, "cancel", inquiryId=cancel_id, sourceSessionId=sa, targetSessionId=sb)["cancelled"]
        assert cancelled.result(15)["status"] == "cancelled"
        model.release.set()
        model.block = False
        cross = b.new(other)
        denied = coord(a, "ask", **dict(ask, inquiryId=inquiry_id(), targetSessionId=cross))
        assert denied["status"] == "rejected" and denied["error"]["code"] == "permission_denied", denied
        b.approve = True
        allowed = coord(a, "ask", **dict(ask, inquiryId=inquiry_id(), targetSessionId=cross))
        assert allowed["status"] == "answered", allowed
        assert len(b.permissions) == 2
        print("PASS cancellation and cross-workspace one-shot approval/rejection", flush=True)

        # Exercise the actual built-in tool path too, not only ACP extensions.
        model.tool_target = sb
        before = len(a.notices)
        assert a.call("session/prompt", {"sessionId": sa, "prompt": [{"type": "text",
                      "text": "COORDINATION_TOOL_FLOW: list active sessions, then ask the target for its status."}]
                      })["stopReason"] == "end_turn"
        updates = [n["params"]["update"] for n in a.notices[before:] if n["method"] == "session/update"]
        for name in ("list_active_sessions", "ask_session"):
            calls = [u for u in updates if u.get("sessionUpdate") == "tool_call" and u.get("title") == name]
            assert len(calls) == 1, (name, updates)
            tool_id = calls[0]["toolCallId"]
            changes = [u for u in updates if u.get("sessionUpdate") == "tool_call_update"
                       and u["toolCallId"] == tool_id]
            # Fast tools may go straight from Pending to Completed. Ask must
            # additionally expose its asynchronous wait via standard updates.
            if name == "ask_session":
                assert any(u.get("status") == "in_progress" for u in changes), (name, changes)
            finished = [u for u in changes if u.get("status") == "completed"]
            assert len(finished) == 1, (name, changes)
            text = "".join(c.get("content", {}).get("text", "") for c in finished[0].get("content", []))
            result_body = json.loads(text)
            if name == "list_active_sessions":
                assert sb in [s["sessionId"] for s in result_body["sessions"]], result_body
            else:
                assert result_body["status"] == "answered", result_body
                assert result_body["answer"] == "COORDINATION_SIDE_ANSWER", result_body
        assert not any("outgoing inquiry" in json.dumps(n) for n in a.notices[before:])
        print("PASS real list/ask tool lifecycle, expandable return content, no source system notices", flush=True)

        # The source's ordinary tool call owns the live UI. Audit-only source
        # records remain durable, but do not inject extra system notifications.
        assert not any(qid in json.dumps(n) and "outgoing inquiry" in json.dumps(n) for n in a.notices)
        for subject, title in (("incoming inquiry", f"Answering session {sa}"),
                               ("inquiry completed", f"Answered session {sa}")):
            eventually(lambda: any(qid in json.dumps(n) and subject in json.dumps(n)
                                   and title in json.dumps(n) for n in b.notices))
        count = len(model.inquiries())
        a.close()
        a = spawn("a-reloaded")
        a.call("session/load", {"sessionId": sa, "cwd": str(cwd), "mcpServers": []})
        assert coord(a, "get", inquiryId=qid, sourceSessionId=sa)["outcome"] == result
        assert coord(a, "ask", **ask) == result
        assert len(model.inquiries()) == count, "reloaded retry repeated inference"
        eventually(lambda: any(qid in json.dumps(n) and "outgoing inquiry completed" in json.dumps(n)
                               for n in a.notices))
        print("PASS symmetric durable audit, reload query, no replay after process restart", flush=True)

        b.proc.kill()
        b.proc.wait(timeout=5)
        eventually(lambda: not coord(a, "list", sourceSessionId=sa)["sessions"], seconds=18)
        print("PASS crashed target leaves no usable ghost session after lease expiry", flush=True)

        def receiver_notices(client, id, start=0):
            return [n for n in client.notices[start:]
                    if n.get("params", {}).get("update", {}).get("sessionUpdate") == "ui_notice"
                    and n["params"]["update"].get("correlationId") == id]

        b = spawn("b-ui-recovery")
        b.call("session/load", {"sessionId": sb, "cwd": str(cwd), "mcpServers": []})
        c = spawn("c")
        sc = c.new(cwd)
        shared_id = inquiry_id()
        one = coord(a, "ask", **dict(ask, inquiryId=shared_id, question="Source A progress?"))
        two = coord(c, "ask", **dict(ask, inquiryId=shared_id, sourceSessionId=sc, question="Source C progress?"))
        assert one["status"] == two["status"] == "answered"
        receipts = [json.loads(n["params"]["update"]["details"]) for n in receiver_notices(b, shared_id)
                    if n["params"]["update"].get("subject") == "incoming inquiry"]
        assert len(receipts) == 2 and len({r["sourcePeerId"] for r in receipts}) == 2, receipts
        assert {r["sourceSessionId"] for r in receipts} == {sa, sc}
        print("PASS same InquiryId from independent peers has distinct durable presentation identity", flush=True)

        model.block = True
        model.release.clear()
        live_id = inquiry_id()
        count = len(model.inquiries())
        live = pool.submit(coord, a, "ask", **dict(ask, inquiryId=live_id, question="Resident reload probe"))
        eventually(lambda: len(model.inquiries()) == count + 1)
        before = len(b.notices)
        b.call("session/load", {"sessionId": sb, "cwd": str(cwd), "mcpServers": []})
        snapshots = [n for n in receiver_notices(b, live_id, before)
                     if n["params"].get("_meta", {}).get("transient")]
        assert snapshots and all(n["params"]["update"]["subject"] == "incoming inquiry" for n in snapshots), snapshots
        assert len(model.inquiries()) == count + 1, "resident reload repeated inference"
        model.release.set()
        live_result = live.result(15)
        assert live_result["status"] == "answered", live_result
        print("PASS resident reload republishes live inquiry without duplicate inference or audit start", flush=True)

        model.release.clear()
        orphan_id, queued_orphan_id = inquiry_id(), inquiry_id()
        count = len(model.inquiries())
        orphan = pool.submit(coord, a, "ask", **dict(ask, inquiryId=orphan_id, question="Crash during answer"))
        eventually(lambda: len(model.inquiries()) == count + 1)
        queued_orphan = pool.submit(coord, c, "ask", **dict(ask, inquiryId=queued_orphan_id,
                                  sourceSessionId=sc, question="Crash while queued"))
        def orphan_is_queued():
            reply = c.call("_grow/coordination/get", {"inquiryId": queued_orphan_id,
                           "sourceSessionId": sc}, raw=True)
            return reply.get("result", {}).get("phase") == "queued"
        eventually(orphan_is_queued)
        b.proc.kill()
        b.proc.wait(timeout=5)
        # Sources are fixture processes too; closing them releases outstanding
        # callers without asking a replacement target to repeat unknown work.
        a.close()
        c.close()
        for pending in (orphan, queued_orphan):
            try:
                pending.result(15)
            except (RuntimeError, AssertionError, EOFError):
                pass
        model.release.set()
        model.block = False
        b = spawn("b-orphan-reloaded")
        for _ in range(2):
            before = len(b.notices)
            b.call("session/load", {"sessionId": sb, "cwd": str(cwd), "mcpServers": []})
            for id in (orphan_id, queued_orphan_id):
                terminals = [n["params"]["update"] for n in receiver_notices(b, id, before)
                             if n["params"]["update"].get("subject") == "inquiry completed"]
                assert len(terminals) == 1, terminals
                audit = json.loads(terminals[0]["details"])
                assert audit["outcome"]["status"] == "unavailable", audit
                assert audit["outcome"]["error"]["code"] == "target_restarted", audit
                assert terminals[0]["message"].startswith("Unable to answer session ")
        assert len(model.inquiries()) == count + 1, "crash recovery repeated an inquiry"
        assert b.call("session/prompt", {"sessionId": sb, "prompt": [{"type": "text",
                      "text": "Continue ordinary work after inquiry recovery."}]})["stopReason"] == "end_turn"
        print("PASS running and queued crash receipts get one durable interrupted terminal across reloads", flush=True)
    finally:
        model.release.set()
        model.foreground_release.set()
        for client in reversed(clients):
            client.close()
        pool.shutdown(wait=True, cancel_futures=True)
        model.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--keep", action="store_true", help="retain isolated fixtures and stderr logs")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if args.keep:
        root = Path(tempfile.mkdtemp(prefix="grow-coordination-e2e-", dir="/tmp" if os.name == "posix" else None))
        print(f"Fixtures: {root}", flush=True)
        run(binary, root)
    else:
        with tempfile.TemporaryDirectory(prefix="grow-coordination-e2e-", dir="/tmp" if os.name == "posix" else None) as directory:
            run(binary, Path(directory))
