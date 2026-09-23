#!/usr/bin/env python3
"""Real-process parent/child agent-message loopback smoke test.

Run with an already-built binary, for example::

    python3 scripts/test_agent_messages.py --binary target/debug/grow

The fixture uses only the stdlib and an isolated GROW_HOME.  The child is
created by the real ``task`` tool; the HTTP server only supplies deterministic
model responses and never calls a paid provider.
"""

import argparse
import http.server
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time

from test_local_coordination import Client, eventually


PRIMARY_MARKER = "AGENT_MESSAGE_PRIMARY_START"
CHILD_MARKER = "AGENT_MESSAGE_CHILD_START"
PARENT_MESSAGE = "AGENT_MESSAGE_PARENT_BODY"
CHILD_REPLY = "AGENT_MESSAGE_CHILD_REPLY"


def nested_values(value, key):
    """Yield values from JSON objects, including JSON encoded tool text."""
    if isinstance(value, dict):
        if key in value:
            yield value[key]
        for child in value.values():
            yield from nested_values(child, key)
    elif isinstance(value, list):
        for child in value:
            yield from nested_values(child, key)
    elif isinstance(value, str) and value.lstrip().startswith(("{", "[")):
        try:
            yield from nested_values(json.loads(value), key)
        except ValueError:
            pass


def first_string(body, *keys):
    for key in keys:
        for value in nested_values(body, key):
            if isinstance(value, str) and value:
                return value
    return None


def message_refs(body):
    refs = []

    def visit(value):
        if isinstance(value, dict):
            source = value.get("source_session_id", value.get("sourceSessionId"))
            message = value.get("message_id", value.get("messageId"))
            if isinstance(source, str) and isinstance(message, str):
                refs.append((source, message))
            for child in value.values():
                visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)
        elif isinstance(value, str) and value.lstrip().startswith(("{", "[")):
            try:
                visit(json.loads(value))
            except ValueError:
                pass

    visit(body)
    return refs


def receive_results(body):
    """Return parsed receive-agent-message result payloads only.

    Provider requests carry the runtime receive call arguments separately from
    the tool result.  Restricting this helper to the complete result shape
    avoids counting schema text or historical serialized arguments as another
    delivery of the same message.
    """
    results = []

    def visit(value):
        if isinstance(value, dict):
            if (isinstance(value.get("receipt_id"), str)
                    and isinstance(value.get("source_session_id"), str)
                    and isinstance(value.get("target_session_id"), str)
                    and isinstance(value.get("message_id"), str)
                    and isinstance(value.get("message"), str)):
                results.append(value)
            for child in value.values():
                visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)
        elif isinstance(value, str) and value.lstrip().startswith(("{", "[")):
            try:
                visit(json.loads(value))
            except ValueError:
                pass

    visit(body)
    return results


def tool_result_payloads(body, call_id):
    """Return parsed results paired with one model-generated tool call id."""
    payloads = []

    def add(value):
        if isinstance(value, dict):
            payloads.append(value)
        elif isinstance(value, str) and value.lstrip().startswith(("{", "[")):
            try:
                parsed = json.loads(value)
            except ValueError:
                return
            if isinstance(parsed, dict):
                payloads.append(parsed)

    def visit(value):
        if isinstance(value, dict):
            if value.get("role") == "tool" and value.get("tool_call_id") == call_id:
                add(value.get("content"))
            elif value.get("type") == "function_call_output" and value.get("call_id") == call_id:
                add(value.get("output"))
            for child in value.values():
                visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)
        elif isinstance(value, str) and value.lstrip().startswith(("{", "[")):
            try:
                visit(json.loads(value))
            except ValueError:
                pass

    visit(body)
    return payloads


def response_events(path, model, tool_call, text):
    model_name = "test-model"
    if path.endswith("/responses"):
        response = {
            "id": "resp_agent_messages",
            "object": "response",
            "created_at": 1234567890,
            "model": model_name,
            "status": "completed",
            "output": [{
                "type": "message", "id": "msg_agent_messages", "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": text, "annotations": []}],
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15},
        }
        events = [{"type": "response.created", "sequence_number": 0,
                   "response": dict(response, output=[], status="in_progress")}]
        if tool_call:
            name, arguments, call_id = tool_call
            item = {"type": "function_call", "id": "fc_" + call_id,
                    "call_id": call_id, "name": name,
                    "arguments": json.dumps(arguments), "status": "completed"}
            response["output"] = [item]
            events.extend([
                {"type": "response.output_item.added", "sequence_number": 1,
                 "output_index": 0, "item": dict(item, arguments="", status="in_progress")},
                {"type": "response.function_call_arguments.delta", "sequence_number": 2,
                 "item_id": item["id"], "output_index": 0,
                 "delta": item["arguments"]},
                {"type": "response.output_item.done", "sequence_number": 3,
                 "output_index": 0, "item": item},
            ])
        events.append({"type": "response.completed", "sequence_number": len(events),
                       "response": response})
        return events

    if tool_call:
        name, arguments, call_id = tool_call
        delta = {"role": "assistant", "tool_calls": [{
            "index": 0, "id": call_id, "type": "function",
            "function": {"name": name, "arguments": json.dumps(arguments)},
        }]}
        return [
            {"id": "chat_agent_messages", "object": "chat.completion.chunk",
             "created": 1234567890, "model": model_name,
             "choices": [{"index": 0, "delta": delta, "finish_reason": "tool_calls"}]},
            {"id": "chat_agent_messages", "object": "chat.completion.chunk",
             "created": 1234567890, "model": model_name, "choices": [],
             "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}},
        ]
    return [
        {"id": "chat_agent_messages", "object": "chat.completion.chunk",
         "created": 1234567890, "model": model_name,
         "choices": [{"index": 0, "delta": {"role": "assistant", "content": text},
                       "finish_reason": "stop"}]},
        {"id": "chat_agent_messages", "object": "chat.completion.chunk",
         "created": 1234567890, "model": model_name, "choices": [],
         "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}},
    ]


class MessageModel(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), MessageModelHandler)
        self.lock = threading.Lock()
        self.requests = []
        self.errors = []
        self.primary_stage = 0
        self.child_stage = 0
        self.primary_session = None
        self.child_session = None
        self.child_release = threading.Event()
        self.reply_received = threading.Event()
        self.send_calls = []
        self.tool_calls = []
        threading.Thread(target=self.serve_forever, daemon=True).start()

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server_port}"

    def close(self):
        self.child_release.set()
        self.reply_received.set()
        self.shutdown()
        self.server_close()


class MessageModelHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
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
        serialized = json.dumps(body, ensure_ascii=False)
        tool_names = [tool.get("name") or tool.get("function", {}).get("name")
                      for tool in body.get("tools", [])]
        # The child-only ask_parent tool is authoritative.  Prompt history can
        # contain the parent's marker after task inheritance, so marker search
        # alone would occasionally misclassify a child request as primary.
        role = "auxiliary" if not tool_names else ("child" if "ask_parent" in tool_names else "primary")
        with self.server.lock:
            self.server.requests.append({"role": role, "body": body, "tools": tool_names})
            if "receive_agent_message" in tool_names:
                self.server.errors.append("historical receive_agent_message was advertised")
        try:
            tool_call, text = self.decide(role, body, serialized, tool_names)
        except (AssertionError, RuntimeError) as error:
            with self.server.lock:
                self.server.errors.append(f"{role}: {error}")
            self.server.child_release.set()
            self.server.reply_received.set()
            tool_call, text = None, f"MODEL_FIXTURE_ERROR: {error}"
        if tool_call:
            name, arguments, call_id = tool_call
            with self.server.lock:
                self.server.tool_calls.append({
                    "role": role, "name": name, "arguments": arguments, "call_id": call_id,
                })
        events = response_events(self.path, self.server, tool_call, text)
        try:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            for event in events:
                self.wfile.write(f"data: {json.dumps(event)}\n\n".encode())
            self.wfile.write(b"data: [DONE]\n\n")
        except (BrokenPipeError, ConnectionResetError):
            pass

    def decide(self, role, body, serialized, tool_names):
        server = self.server
        if role == "auxiliary":
            return None, json.dumps({"session_title": "Agent message test"})
        if role == "primary":
            if server.primary_stage == 0:
                task_name = next((name for name in ("spawn_subagent", "task") if name in tool_names), None)
                assert task_name, tool_names
                server.primary_stage = 1
                return (task_name, {
                    "description": "Inspect the isolated workspace safely",
                    "prompt": f"{CHILD_MARKER}: wait for a parent message, then list the workspace and reply.",
                    "subagent_type": "explore",
                    "capability_mode": "read-only",
                    "run_in_background": True,
                }, "primary_task"), ""
            if server.primary_stage == 1:
                task_results = [item.get("content", item.get("output", ""))
                                for item in body.get("messages", body.get("input", []))
                                if item.get("tool_call_id", item.get("call_id")) == "primary_task"
                                and (item.get("role") == "tool" or item.get("type") == "function_call_output")]
                child_match = re.search(r"(?m)^subagent_id: ([^\s]+)$", "\n".join(task_results))
                child_id = child_match.group(1) if child_match else None
                assert child_id and child_id != "read-only", serialized[-4000:]
                server.child_session = child_id
                assert "send_subagent_message" in tool_names, tool_names
                server.primary_stage = 2
                return ("send_subagent_message", {
                    "subagent_id": child_id, "message": PARENT_MESSAGE, "interrupt": False,
                }, "primary_send"), ""
            if server.primary_stage == 2:
                ack = tool_result_payloads(body, "primary_send")
                assert len(ack) == 1 and ack[0].get("status") == "received", ack
                assert isinstance(ack[0].get("receipt_id"), str) and ack[0]["receipt_id"], ack
                server.send_calls.append({"role": "primary", "call_id": "primary_send", "body": body})
                server.child_release.set()
                if not server.reply_received.wait(30):
                    raise RuntimeError("child reply did not arrive after parent receipt")
                assert "list_dir" in tool_names, tool_names
                server.primary_stage = 3
                return ("list_dir", {"target_directory": "."}, "primary_list"), ""
            assert server.primary_stage == 3
            assert "target_directory" in serialized, serialized[-5000:]
            replies = receive_results(body)
            assert sum(result["message"] == CHILD_REPLY for result in replies) == 1, replies
            server.primary_stage = 4
            return None, "PRIMARY_SAFE_COMPLETE"

        if server.child_stage == 0:
            assert "ask_parent" in tool_names, tool_names
            server.child_stage = 1
            if not server.child_release.wait(30):
                raise RuntimeError("child was not released by parent receipt")
            assert "send_subagent_message" in tool_names, tool_names
            return ("list_dir", {"target_directory": "."}, "child_list"), ""
        if server.child_stage == 1:
            assert "target_directory" in serialized, serialized[-5000:]
            incoming = receive_results(body)
            assert sum(result["message"] == PARENT_MESSAGE for result in incoming) == 1, incoming
            refs = message_refs(body)
            assert refs, "child history did not contain source/message reference"
            source, message = next(
                ((source, message) for source, message in refs
                 if server.primary_session is None or source == server.primary_session),
                refs[0],
            )
            assert source and message
            server.child_stage = 2
            return ("send_subagent_message", {
                "reply_to": {"source_session_id": source, "message_id": message},
                "message": CHILD_REPLY, "interrupt": False,
            }, "child_reply"), ""
        assert server.child_stage == 2
        ack = tool_result_payloads(body, "child_reply")
        assert len(ack) == 1 and ack[0].get("status") == "received", ack
        assert isinstance(ack[0].get("receipt_id"), str) and ack[0]["receipt_id"], ack
        server.send_calls.append({"role": "child", "call_id": "child_reply", "body": body})
        server.reply_received.set()
        server.child_stage = 3
        return None, "CHILD_SAFE_COMPLETE"


def run(binary, root):
    model = MessageModel()
    client = None
    failed = False
    stderr_path = root / "primary.stderr.log"
    try:
        home = root / "grow-home"
        workspace = root / "workspace"
        home.mkdir()
        workspace.mkdir()
        subprocess.run(["git", "init", "-q", str(workspace)], check=True)
        (home / "config.toml").write_text(f'''[models]
default = "mock/test-model"
[provider.mock.options]
base_url = "{model.url}"
[provider.mock.models.test-model]
context_window = 200000
''')
        env = dict(os.environ, GROW_HOME=str(home), GROW_API_KEY="agent-messages-test-key",
                   GROW_DISABLE_AUTO_UPDATE="1", NO_PROXY="127.0.0.1,localhost")
        for name in ("GROW_CLI_CHAT_PROXY_BASE_URL", "GROW_INFERENCE_BASE_URL",
                     "GROW_MODELS_BASE_URL", "GROW_FEEDBACK_BASE_URL",
                     "GROW_CONVERSATIONS_BASE_URL"):
            env[name] = model.url
        client = Client(binary, workspace, env, stderr_path)
        client.init()
        primary = client.new(workspace)
        model.primary_session = primary
        result = client.call("session/prompt", {
            "sessionId": primary,
            "prompt": [{"type": "text", "text":
                         f"{PRIMARY_MARKER}: create one read-only explore child and coordinate a reply."}],
        }, timeout=90)
        assert result.get("stopReason") == "end_turn", result
        eventually(lambda: model.primary_stage == 4 and model.child_stage == 3, seconds=10)
        assert not model.errors, model.errors
        assert len(model.send_calls) == 2, model.send_calls
        assert {call["role"] for call in model.send_calls} == {"primary", "child"}
        requests = model.requests
        assert any(r["role"] == "child" for r in requests)
        assert all("receive_agent_message" not in r["tools"] for r in requests)
        assert model.primary_stage == 4 and model.child_stage == 3

        task_requests = [r for r in requests if r["role"] == "primary" and any(name in r["tools"] for name in ("spawn_subagent", "task"))]
        assert task_requests, "primary never received task tool"
        task_calls = [call for call in model.tool_calls if call["call_id"] == "primary_task"]
        assert len(task_calls) == 1, task_calls
        task_arguments = task_calls[0]["arguments"]
        assert task_arguments["capability_mode"] == "read-only", task_arguments
        assert task_arguments["subagent_type"] == "explore", task_arguments
        assert task_arguments["run_in_background"] is True, task_arguments
        sends = [r for r in requests if "send_subagent_message" in r["tools"]]
        assert sends, "send tool was never available"
        send_calls = [call for call in model.tool_calls if call["name"] == "send_subagent_message"]
        assert {call["call_id"] for call in send_calls} == {"primary_send", "child_reply"}, send_calls
        assert {
            call["call_id"]: call["arguments"]["message"]
            for call in send_calls
        } == {"primary_send": PARENT_MESSAGE, "child_reply": CHILD_REPLY}, send_calls
        receipt_ids = []
        for call in model.send_calls:
            payloads = tool_result_payloads(call["body"], call["call_id"])
            assert len(payloads) == 1, (call["call_id"], payloads)
            assert payloads[0].get("status") == "received", payloads
            receipt_id = payloads[0].get("receipt_id")
            assert isinstance(receipt_id, str) and receipt_id, payloads
            receipt_ids.append(receipt_id)
        assert len(receipt_ids) == len(set(receipt_ids)) == 2, receipt_ids
        child_reply_calls = [call for call in model.send_calls if call["role"] == "child"]
        assert len(child_reply_calls) == 1
        reply_refs = message_refs(child_reply_calls[0]["body"])
        assert any(source == primary for source, _message in reply_refs), reply_refs
        child_receive_requests = [
            receive_results(r["body"])
            for r in requests
            if r["role"] == "child" and receive_results(r["body"])
        ]
        assert child_receive_requests, "child never received a structured parent message"
        assert all(
            sum(result["message"] == PARENT_MESSAGE for result in results) == 1
            for results in child_receive_requests
        ), child_receive_requests
        assert all(
            len({result["receipt_id"] for result in results}) == len(results)
            for results in child_receive_requests
        ), child_receive_requests
        primary_receive_requests = [
            receive_results(r["body"])
            for r in requests
            if r["role"] == "primary" and receive_results(r["body"])
        ]
        assert primary_receive_requests, "parent never received a structured child reply"
        assert all(
            sum(result["message"] == CHILD_REPLY for result in results) == 1
            for results in primary_receive_requests
        ), primary_receive_requests
        assert all(
            len({result["receipt_id"] for result in results}) == len(results)
            for results in primary_receive_requests
        ), primary_receive_requests
        assert len([call for call in model.tool_calls if call["call_id"] == "primary_send"]) == 1
        assert len([call for call in model.tool_calls if call["call_id"] == "child_reply"]) == 1
        assert not client.permissions, client.permissions
        print("PASS real task/read-only parent↔child send/reply loopback", flush=True)
        client.close()
        request_count = len(model.requests)
        client = Client(binary, workspace, env, root / "reloaded.stderr.log")
        client.init()
        client.call("session/load", {"sessionId": primary, "cwd": str(workspace), "mcpServers": []})
        reply_receipt = receipt_ids[1]
        restored = eventually(lambda: [
            notice for notice in client.notices
            if notice.get("params", {}).get("update", {}).get("subject") == "agent message received"
            and notice["params"]["update"].get("correlationId") == reply_receipt
        ])
        assert len(restored) == 1, restored
        assert CHILD_REPLY in json.dumps(restored), restored
        assert len(model.requests) == request_count, "cold replay started another model request"
        print("PASS consumed reply cold-reloads once without inference or redelivery", flush=True)
    except Exception:
        failed = True
        raise
    finally:
        if client is not None:
            client.close()
        model.close()
        if failed:
            print(f"model errors: {model.errors!r}", file=sys.stderr)
            print(f"model stages: primary={model.primary_stage} child={model.child_stage}", file=sys.stderr)
            with model.lock:
                print("model requests:", file=sys.stderr)
                for request in model.requests[-12:]:
                    print(json.dumps(request, ensure_ascii=False), file=sys.stderr)
            if stderr_path.exists():
                print("primary stderr:", file=sys.stderr)
                print(stderr_path.read_text(errors="replace")[-16000:], file=sys.stderr)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--keep", action="store_true", help="retain isolated fixtures and logs")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if args.keep:
        root = Path(tempfile.mkdtemp(prefix="grow-agent-messages-", dir="/tmp" if os.name == "posix" else None))
        print(f"Fixtures: {root}", flush=True)
        run(binary, root)
    else:
        with tempfile.TemporaryDirectory(prefix="grow-agent-messages-", dir="/tmp" if os.name == "posix" else None) as directory:
            run(binary, Path(directory))
