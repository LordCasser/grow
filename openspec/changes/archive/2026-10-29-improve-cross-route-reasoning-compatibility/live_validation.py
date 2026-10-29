#!/usr/bin/env python3
"""Run the private, real-provider ACP validation matrix for this change.

This driver deliberately uses only the Python standard library. It never prints
configuration, RPC traffic, notifications, stderr, or credentials. Keep the
temporary run directory private; pass --keep-private-run when its raw evidence
and isolated session tree need to be inspected after the run.
"""

from __future__ import annotations

import argparse
import datetime as _datetime
import json
import os
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
from pathlib import Path
from typing import Any


RPC_TIMEOUT_SECONDS = 180
MAX_RETRIES = 3
ROUTE_MATRICES = {
    "primary": (
        ("opencode/glm-5.3-flash", "chat_completions"),
        ("opencode/deepseek-v4.1-flash", "chat_completions"),
        ("neorouter/deepseek-flash", "responses"),
        ("qwen2/qwen3.8-flash", "messages"),
        ("opencode/deepseek-v4.1-flash", "chat_completions"),
        ("qwen2/qwen3.8-flash", "messages"),
        ("neorouter/deepseek-flash", "responses"),
        ("opencode/deepseek-v4.1-flash", "chat_completions"),
        ("bigmodel/glm-5.3-flash", "chat_completions"),
    ),
    "direct": (
        ("opencode/glm-5.3-flash", "chat_completions"),
        ("opencode/deepseek-v4.1-flash", "chat_completions"),
        ("deepseek/deepseek-flash", "responses"),
        ("opencode/qwen3.8-max", "messages"),
        ("opencode/deepseek-v4.1-flash", "chat_completions"),
        ("opencode/qwen3.8-max", "messages"),
        ("deepseek/deepseek-flash", "responses"),
        ("opencode/deepseek-v4.1-flash", "chat_completions"),
        ("bigmodel/glm-5.3-flash", "chat_completions"),
    ),
}
HISTORY_SOURCE = "opencode/deepseek-v4.1-flash"
HISTORY_TARGETS = {
    "primary": "neorouter/deepseek-flash",
    "direct": "deepseek/deepseek-flash",
}


class ValidationError(RuntimeError):
    pass


class RpcTimeout(TimeoutError):
    def __init__(self, method: str, request_id: int):
        super().__init__(f"{method} exceeded {RPC_TIMEOUT_SECONDS}s")
        self.method = method
        self.request_id = request_id


def _toml_key(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def _toml_value(value: Any) -> str:
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return repr(value)
    if isinstance(value, (_datetime.datetime, _datetime.date, _datetime.time)):
        return value.isoformat()
    if isinstance(value, list):
        return "[" + ", ".join(_toml_value(item) for item in value) + "]"
    if isinstance(value, dict):
        pairs = (f"{_toml_key(str(k))} = {_toml_value(v)}" for k, v in value.items())
        return "{ " + ", ".join(pairs) + " }"
    raise ValidationError(f"unsupported TOML value type: {type(value).__name__}")


def _serialize_table(path: tuple[str, ...], table: dict[str, Any]) -> list[str]:
    lines = ["[" + ".".join(_toml_key(part) for part in path) + "]"]
    children: list[tuple[str, dict[str, Any]]] = []
    for key, value in table.items():
        if isinstance(value, dict):
            children.append((key, value))
        else:
            lines.append(f"{_toml_key(str(key))} = {_toml_value(value)}")
    for key, child in children:
        lines.append("")
        lines.extend(_serialize_table((*path, key), child))
    return lines


def serialize_config(config: dict[str, Any]) -> str:
    lines: list[str] = []
    for key, value in config.items():
        if isinstance(value, dict):
            lines.extend(_serialize_table((key,), value))
            lines.append("")
        else:
            lines.append(f"{_toml_key(key)} = {_toml_value(value)}")
    return "\n".join(lines).rstrip() + "\n"


def collect_secrets(value: Any, key: str = "") -> tuple[list[str], set[str]]:
    secrets: list[str] = []
    env_names: set[str] = set()
    sensitive = re.compile(r"(api.?key|secret|token|password|credential|authorization|private.?key|access.?key)", re.I)

    def visit(node: Any, name: str) -> None:
        if isinstance(node, dict):
            if name.lower() == "env_http_headers":
                env_names.update(str(v) for v in node.values() if isinstance(v, str))
            for child_name, child in node.items():
                visit(child, str(child_name))
            return
        if isinstance(node, list):
            for child in node:
                visit(child, name)
            return
        if name.lower() in {"env_key", "env_http_headers"}:
            if name.lower() == "env_key" and isinstance(node, str):
                env_names.add(node)
            elif name.lower() == "env_key" and isinstance(node, list):
                env_names.update(item for item in node if isinstance(item, str))
            elif name.lower() == "env_http_headers" and isinstance(node, dict):
                env_names.update(str(v) for v in node.values() if isinstance(v, str))
        if sensitive.search(name) and isinstance(node, str) and node:
            secrets.append(node)

    visit(value, key)
    return secrets, env_names


class Redactor:
    def __init__(self, secrets: list[str]):
        self.secrets = sorted({s for s in secrets if s}, key=len, reverse=True)

    def text(self, value: str) -> str:
        for secret in self.secrets:
            value = value.replace(secret, "[REDACTED]")
        value = re.sub(r"(?i)\bBearer\s+[^\s,;\"']+", "Bearer [REDACTED]", value)
        value = re.sub(r"\bsk-[A-Za-z0-9_-]{8,}\b", "[REDACTED]", value)
        return value

    def value(self, obj: Any) -> Any:
        if isinstance(obj, str):
            return self.text(obj)
        if isinstance(obj, list):
            return [self.value(item) for item in obj]
        if isinstance(obj, dict):
            return {self.text(str(k)): self.value(v) for k, v in obj.items()}
        return obj


def selected_config(
    source: dict[str, Any],
    routes: tuple[tuple[str, str], ...],
    extra_routes: tuple[str, ...] = (),
    high_where_configured: bool = False,
) -> dict[str, Any]:
    providers = source.get("provider")
    if not isinstance(providers, dict):
        raise ValidationError("source config has no [provider.*] tables")

    route_models: dict[str, dict[str, Any]] = {}
    selected_providers: dict[str, dict[str, Any]] = {}
    route_ids = list(dict.fromkeys([route for route, _ in routes] + list(extra_routes)))
    for route in route_ids:
        provider_id, model_id = route.split("/", 1)
        provider = providers.get(provider_id)
        if not isinstance(provider, dict):
            raise ValidationError(f"source config lacks provider `{provider_id}`")
        model_table = provider.get("models")
        model = model_table.get(model_id) if isinstance(model_table, dict) else None
        if not isinstance(model, dict):
            raise ValidationError(f"source config lacks model `{route}`")
        route_models[route] = dict(model)
        if high_where_configured:
            efforts = route_models[route].get("reasoning_efforts")
            has_high = isinstance(efforts, list) and any(
                isinstance(effort, dict)
                and (str(effort.get("value", "")).lower() == "high"
                     or str(effort.get("id", "")).lower() == "high")
                for effort in efforts
            )
            if has_high:
                for effort in efforts:
                    if isinstance(effort, dict):
                        is_high = str(effort.get("value", "")).lower() == "high" or str(effort.get("id", "")).lower() == "high"
                        effort["default"] = is_high
        route_models[route]["max_retries"] = MAX_RETRIES
        chosen = selected_providers.setdefault(provider_id, {})
        for key, value in provider.items():
            if key not in {"models", "options"}:
                chosen[key] = value
        if "options" in provider:
            chosen["options"] = provider["options"]
        chosen.setdefault("models", {})[model_id] = route_models[route]

    # Do not copy arbitrary [models] settings: they may reference unselected
    # models (for example image_description) or change test routing. Each
    # selected model keeps its own reasoning default from its copied table.
    output: dict[str, Any] = {
        "models": {"default": routes[0][0]},
        "provider": selected_providers,
    }
    return output


def write_private(path: Path, content: str | bytes, mode: int = 0o600) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
    fd = os.open(path, flags, mode)
    try:
        with os.fdopen(fd, "wb") as stream:
            data = content.encode("utf-8") if isinstance(content, str) else content
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
    except BaseException:
        raise


def build_environment(private_home: Path, config: dict[str, Any]) -> dict[str, str]:
    secrets, env_names = collect_secrets(config)
    del secrets
    env: dict[str, str] = {}
    for name in ("PATH", "LANG", "LC_ALL", "LC_CTYPE", "SSL_CERT_FILE", "SSL_CERT_DIR",
                 "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "http_proxy",
                 "https_proxy", "all_proxy", "no_proxy"):
        if name in os.environ:
            env[name] = os.environ[name]
    for name in env_names:
        if name in os.environ:
            env[name] = os.environ[name]
    env["HOME"] = str(private_home)
    env["USERPROFILE"] = str(private_home)
    env["TMPDIR"] = str(private_home / "tmp")
    env["TMP"] = env["TEMP"] = str(private_home / "tmp")
    env["GROW_HOME"] = str(private_home / "grow")
    env["GROW_LOG"] = "unified"
    env["GROW_DISABLE_AUTOUPDATER"] = "1"
    env["GROW_FEEDBACK_ENABLED"] = "false"
    return env


class RpcClient:
    def __init__(self, binary: Path, cwd: Path, env: dict[str, str], private: Path):
        self.process = subprocess.Popen(
            [str(binary), "agent", "stdio"], cwd=cwd, env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8", errors="replace", bufsize=1,
        )
        self.inbox: queue.Queue[Any] = queue.Queue()
        self.write_lock = threading.Lock()
        self.raw_lock = threading.Lock()
        self.next_id = 1
        self.notifications: list[dict[str, Any]] = []
        self.raw_file = open(private / "rpc-raw.jsonl", "a", encoding="utf-8", buffering=1)
        os.chmod(private / "rpc-raw.jsonl", 0o600)
        self.stderr_file = open(private / "stderr.log", "a", encoding="utf-8", buffering=1)
        os.chmod(private / "stderr.log", 0o600)
        self.reader_threads = [
            threading.Thread(target=self._read_stdout, name="grow-acp-stdout", daemon=True),
            threading.Thread(target=self._read_stderr, name="grow-acp-stderr", daemon=True),
        ]
        for thread in self.reader_threads:
            thread.start()

    def _read_stdout(self) -> None:
        assert self.process.stdout is not None
        for line in self.process.stdout:
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                with self.raw_lock:
                    self.raw_file.write(json.dumps({"unparsed": line.rstrip("\n")}, ensure_ascii=False) + "\n")
                continue
            with self.raw_lock:
                self.raw_file.write(json.dumps(message, ensure_ascii=False) + "\n")
            self.inbox.put(message)
        self.inbox.put(EOFError("grow stdio closed stdout"))

    def _read_stderr(self) -> None:
        assert self.process.stderr is not None
        for line in self.process.stderr:
            self.stderr_file.write(line)

    def _send(self, message: dict[str, Any]) -> None:
        if self.process.poll() is not None:
            raise ValidationError(f"grow agent exited with status {self.process.returncode}")
        assert self.process.stdin is not None
        wire = json.dumps(message, ensure_ascii=False, separators=(",", ":"))
        with self.write_lock:
            self.process.stdin.write(wire + "\n")
            self.process.stdin.flush()
        with self.raw_lock:
            self.raw_file.write(json.dumps({"client": message}, ensure_ascii=False) + "\n")

    def _respond_permission(self, message: dict[str, Any]) -> None:
        params = message.get("params") or {}
        options = params.get("options") or []
        selected = next((o for o in options if o.get("kind") == "allow_once"), None)
        if selected is None:
            result = {"outcome": {"outcome": "cancelled"}}
        else:
            result = {"outcome": {"outcome": "selected", "optionId": selected["optionId"]}}
        self._send({"jsonrpc": "2.0", "id": message["id"], "result": result})

    def _dispatch(self, message: Any) -> dict[str, Any] | None:
        if isinstance(message, BaseException):
            raise ValidationError(str(message))
        if "method" in message:
            self.notifications.append(message)
            if message["method"] == "session/request_permission" and "id" in message:
                self._respond_permission(message)
            return None
        return message

    def _wait_response(self, request_id: int, deadline: float) -> dict[str, Any] | None:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            try:
                message = self.inbox.get(timeout=remaining)
            except queue.Empty:
                return None
            response = self._dispatch(message)
            if response is not None and response.get("id") == request_id:
                return response
            if response is not None:
                self.notifications.append({"orphan_response": response})

    def rpc(self, method: str, params: dict[str, Any]) -> tuple[dict[str, Any], float]:
        request_id = self.next_id
        self.next_id += 1
        started = time.monotonic()
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        response = self._wait_response(request_id, started + RPC_TIMEOUT_SECONDS)
        if response is None:
            raise RpcTimeout(method, request_id)
        return response, round((time.monotonic() - started) * 1000)

    def cancel_and_settle_prompt(self, session_id: str, request_id: int) -> tuple[dict[str, Any] | None, float]:
        started = time.monotonic()
        self._send({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session_id}})
        response = self._wait_response(request_id, time.monotonic() + 10)
        return response, round((time.monotonic() - started) * 1000)

    def close(self) -> None:
        if self.process.poll() is None:
            try:
                self.process.stdin.close()  # type: ignore[union-attr]
            except OSError:
                pass
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait()
        for thread in self.reader_threads:
            thread.join(timeout=5)
        for stream in (self.raw_file, self.stderr_file):
            try:
                stream.close()
            except OSError:
                pass


def _extract_text(notification: dict[str, Any]) -> str | None:
    method = notification.get("method")
    if method not in {"session/update", "grow/session/update", "grow/session_notification"}:
        return None
    params = notification.get("params") or {}
    update = params.get("update")
    if not isinstance(update, dict):
        update = params
    tag = update.get("sessionUpdate", update.get("type"))
    if tag not in {"agent_message_chunk", "agentMessageChunk"}:
        return None
    content = update.get("content")
    if isinstance(content, dict) and content.get("type") == "text":
        value = content.get("text")
        return value if isinstance(value, str) else None
    # Some Grow extensions wrap ACP updates as a nested serialized object.
    for child in update.values():
        if isinstance(child, dict) and child.get("type") == "text" and isinstance(child.get("text"), str):
            return child["text"]
    return None


def _is_tool_call(notification: dict[str, Any], session_id: str) -> bool:
    if notification.get("method") not in {"session/update", "grow/session/update", "grow/session_notification"}:
        return False
    params = notification.get("params") or {}
    if params.get("sessionId") not in {None, session_id}:
        return False
    update = params.get("update")
    if not isinstance(update, dict):
        update = params
    return update.get("sessionUpdate", update.get("type")) in {"tool_call", "toolCall"}


def _tool_call_ids(notifications: list[dict[str, Any]], session_id: str) -> set[str]:
    ids: set[str] = set()
    anonymous = 0
    for notification in notifications:
        if not _is_tool_call(notification, session_id):
            continue
        params = notification.get("params") or {}
        update = params.get("update") if isinstance(params.get("update"), dict) else params
        call_id = update.get("toolCallId", update.get("tool_call_id", update.get("id")))
        if isinstance(call_id, str):
            ids.add(call_id)
        else:
            anonymous += 1
    return ids | {f"anonymous-{n}" for n in range(anonymous)}


def _result_ok(response: dict[str, Any]) -> bool:
    return "error" not in response


def _execution_records(path: Path, stage: int) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    records: list[dict[str, Any]] = []
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            if record.get("stage") == stage:
                records.append(record)
    return records


def _safe_rpc(response: dict[str, Any], redactor: Redactor) -> dict[str, Any]:
    return redactor.value(response)


def run(args: argparse.Namespace) -> int:
    routes = ROUTE_MATRICES[args.matrix]
    history_source = args.history_source or HISTORY_SOURCE
    history_target = args.history_target or HISTORY_TARGETS[args.matrix]
    binary = Path(args.binary).expanduser().resolve()
    source_path = Path(args.source_config).expanduser().resolve()
    output_path = Path(args.output).expanduser().resolve()
    if not binary.is_file():
        raise ValidationError("--binary must point to an existing file")
    if not source_path.is_file():
        raise ValidationError("--source-config must point to an existing file")
    if output_path == source_path or output_path == binary:
        raise ValidationError("--output must be separate from the binary and source config")

    with source_path.open("rb") as stream:
        source = tomllib.load(stream)
    minimal = selected_config(
        source, routes, (history_source, history_target), high_where_configured=args.matrix == "direct"
    )
    secrets, _ = collect_secrets(source)
    _selected_secrets, env_names = collect_secrets(minimal)
    secrets.extend(os.environ[name] for name in env_names if os.environ.get(name))
    redactor = Redactor(secrets)

    run_dir = Path(tempfile.mkdtemp(prefix="grow-reasoning-validation-"))
    os.chmod(run_dir, 0o700)
    home = run_dir / "home"
    grow_home = home / "grow"
    temp_dir = home / "tmp"
    workspace = run_dir / "workspace"
    for directory in (home, grow_home, temp_dir, workspace):
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        os.chmod(directory, 0o700)
    config_text = serialize_config(minimal)
    write_private(grow_home / "config.toml", config_text)
    write_private(workspace / "record_step.py", RECORD_STEP_SCRIPT, 0o700)
    executions_path = workspace / "executions.jsonl"
    environment = build_environment(home, minimal)
    # Use only credential variables named by the selected routes.
    for name in env_names:
        if name in os.environ:
            environment[name] = os.environ[name]

    summary: dict[str, Any] = {
        "run_dir": str(run_dir),
        "retained_private_run": bool(args.keep_private_run),
        "matrix": args.matrix,
        "route_sequence": list(routes),
        "reasoning_effort": "high_where_configured" if args.matrix == "direct" else "each_model_default",
        "rpc_timeout_seconds": RPC_TIMEOUT_SECONDS,
        "max_retries": MAX_RETRIES,
        "stages": [],
        "history_session": None,
    }

    client: RpcClient | None = None
    try:
        print("[1/4] starting isolated grow agent stdio", flush=True)
        client = RpcClient(binary, workspace, environment, run_dir)
        init_rpc, init_ms = client.rpc("initialize", {
            "protocolVersion": 1,
            "clientCapabilities": {"fs": {}, "terminal": False},
            "_meta": {"startupHints": {"nonInteractive": True, "skipGitStatus": True,
                                          "skipProjectLayout": True},
                       "clientType": "live-reasoning-validation", "clientVersion": "1"},
        })
        init_result = init_rpc.get("result") or {}
        auth_methods = init_result.get("authMethods") or []
        auth_method = next((m.get("id") for m in auth_methods if m.get("id") == "provider.api_key"), None)
        if auth_method is None:
            raise ValidationError("initialize did not advertise provider.api_key authentication")
        auth_rpc, auth_ms = client.rpc("authenticate", {"methodId": auth_method, "_meta": {"headless": True}})
        if not _result_ok(auth_rpc):
            raise ValidationError("authenticate returned an RPC error")
        summary["initialize"] = {"rpc": _safe_rpc(init_rpc, redactor), "duration_ms": init_ms}
        summary["authenticate"] = {"rpc": _safe_rpc(auth_rpc, redactor), "duration_ms": auth_ms}

        print("[2/4] creating resident session and running route matrix", flush=True)
        first_route = routes[0][0]
        new_rpc, new_ms = client.rpc("session/new", {
            "cwd": str(workspace), "mcpServers": [], "_meta": {"modelId": first_route},
        })
        if not _result_ok(new_rpc):
            raise ValidationError("session/new returned an RPC error")
        session_id = (new_rpc.get("result") or {}).get("sessionId")
        if not isinstance(session_id, str):
            raise ValidationError("session/new did not return sessionId")
        summary["resident_session"] = {
            "session_id": session_id,
            "pid": client.process.pid,
            "new_rpc": _safe_rpc(new_rpc, redactor),
            "duration_ms": new_ms,
        }

        last_successful_token: str | None = None
        last_applied_route: str | None = first_route
        stop_matrix = False
        for index, (target, _backend) in enumerate(routes, start=1):
            if stop_matrix:
                summary["stages"].append({"stage": index, "expected_from": last_applied_route,
                                          "target": target, "status": "not_run",
                                          "reason": "previous prompt did not settle after cancellation"})
                continue
            expected_source = last_applied_route
            print(f"[matrix {index}/9] {expected_source or 'unknown current route'} -> {target}", flush=True)
            stage: dict[str, Any] = {"stage": index, "expected_from": expected_source, "target": target}
            started = time.monotonic()
            try:
                if index > 1:
                    switch_rpc, switch_ms = client.rpc("session/set_config_option", {
                        "sessionId": session_id, "configId": "model", "value": target,
                    })
                    stage["set_config_option"] = {"rpc": _safe_rpc(switch_rpc, redactor), "duration_ms": switch_ms}
                    if not _result_ok(switch_rpc):
                        stage["status"] = "switch_failed"
                        stage["error"] = _rpc_error_message(switch_rpc, redactor)
                        stage["duration_ms"] = round((time.monotonic() - started) * 1000)
                        summary["stages"].append(stage)
                        print(f"[matrix {index}/9] switch failed; recorded and continuing", flush=True)
                        continue
                    last_applied_route = target

                stage["resident_session_id"] = session_id
                stage["process_pid"] = client.process.pid
                prompt_text = (
                    f"For this validation stage, use the available command tool exactly once to run "
                    f"`python3 record_step.py {index}`. Do not run any other tool command. "
                    "After the command completes, answer with the exact current token and the exact token "
                    "from the immediately preceding successful tool run in this conversation, using lines "
                    "CURRENT_TOKEN=<token> and PREVIOUS_TOKEN=<token>. If this is the first successful "
                    "tool run, omit PREVIOUS_TOKEN. Do not guess or fabricate either token."
                )
                notification_start = len(client.notifications)
                prompt_rpc, prompt_ms = client.rpc("session/prompt", {
                    "sessionId": session_id,
                    "prompt": [{"type": "text", "text": prompt_text}],
                })
                stage["prompt_rpc"] = {"rpc": _safe_rpc(prompt_rpc, redactor), "duration_ms": prompt_ms}
                if not _result_ok(prompt_rpc):
                    stage["status"] = "prompt_failed"
                    stage["error"] = _rpc_error_message(prompt_rpc, redactor)
                    records = _execution_records(executions_path, index)
                    stage["tool_execution_count"] = len(records)
                    stage["tool_call_count"] = len(_tool_call_ids(client.notifications[notification_start:], session_id))
                    summary["stages"].append(stage)
                    continue

                text = "".join(filter(None, (_extract_text(n) for n in client.notifications[notification_start:])))
                tool_call_count = len(_tool_call_ids(client.notifications[notification_start:], session_id))
                records = _execution_records(executions_path, index)
                current_token = records[0].get("token") if len(records) == 1 else None
                checks = {
                    "exactly_one_tool_execution": len(records) == 1,
                    "exactly_one_tool_call": tool_call_count == 1,
                    "reported_current_token": bool(current_token and current_token in text),
                    "reported_previous_token": (last_successful_token in text) if last_successful_token else None,
                    "previous_token_was_not_in_user_prompt": True,
                }
                if len(records) == 1 and isinstance(current_token, str):
                    last_successful_token = current_token
                stage.update({
                    "assistant_text": redactor.text(text),
                    "rpc_notifications_count": len(client.notifications) - notification_start,
                    "tool_execution_count": len(records),
                    "tool_call_count": tool_call_count,
                    "checks": checks,
                    "duration_ms": round((time.monotonic() - started) * 1000),
                    "status": "passed" if all(value is True for value in checks.values() if value is not None) else "failed",
                })
                if len(records) == 1:
                    stage["tool_token"] = redactor.text(str(current_token))
                summary["stages"].append(stage)
                print(f"[matrix {index}/9] {stage['status']}; tool executions={len(records)}", flush=True)
            except RpcTimeout as error:
                stage["status"] = "failed"
                stage["error"] = redactor.text(str(error))
                stage["duration_ms"] = round((time.monotonic() - started) * 1000)
                if error.method == "session/prompt":
                    records = _execution_records(executions_path, index)
                    if len(records) == 1 and isinstance(records[0].get("token"), str):
                        last_successful_token = records[0]["token"]
                    stage["tool_execution_count"] = len(records)
                    stage["tool_call_count"] = len(_tool_call_ids(client.notifications[notification_start:], session_id))
                    text = "".join(filter(None, (_extract_text(n) for n in client.notifications[notification_start:])))
                    if text:
                        stage["assistant_text"] = redactor.text(text)
                    settle_error = None
                    try:
                        settled, settle_ms = client.cancel_and_settle_prompt(session_id, error.request_id)
                    except (ValidationError, OSError) as cancel_error:
                        settled, settle_ms = None, 0
                        settle_error = redactor.text(str(cancel_error))
                    stage["cancel_and_settle"] = {
                        "settled": settled is not None,
                        "duration_ms": settle_ms,
                        "rpc": _safe_rpc(settled, redactor) if settled is not None else None,
                    }
                    if settle_error:
                        stage["cancel_and_settle"]["error"] = settle_error
                    if settled is None:
                        stage["status"] = "prompt_timeout_unsettled"
                        stop_matrix = True
                    else:
                        stage["status"] = "prompt_timeout_cancelled_settled"
                elif error.method == "session/set_config_option":
                    last_applied_route = None
                summary["stages"].append(stage)
                print(f"[matrix {index}/9] failed; recorded and continuing", flush=True)
            except (TimeoutError, ValidationError, OSError) as error:
                stage["status"] = "failed"
                stage["error"] = redactor.text(str(error))
                stage["duration_ms"] = round((time.monotonic() - started) * 1000)
                summary["stages"].append(stage)
                print(f"[matrix {index}/9] failed; recorded and continuing", flush=True)

        print("[3/4] checking ordinary assistant history in a second session", flush=True)
        history: dict[str, Any] = {"source": history_source, "target": history_target}
        try:
            if stop_matrix:
                raise ValidationError("skipped because prior prompt did not settle")
            history_new, new_history_ms = client.rpc("session/new", {
                "cwd": str(workspace), "mcpServers": [],
                "_meta": {"modelId": history_source},
            })
            history_sid = (history_new.get("result") or {}).get("sessionId")
            history["new_rpc"] = _safe_rpc(history_new, redactor)
            history["new_duration_ms"] = new_history_ms
            if not _result_ok(history_new) or not isinstance(history_sid, str):
                raise ValidationError("history session/new failed")
            executions_before_history = _execution_summary(executions_path)["total"]
            first_text = ""; second_text = ""
            start = len(client.notifications)
            first_rpc, first_ms = client.rpc("session/prompt", {
                "sessionId": history_sid,
                "prompt": [{"type": "text", "text": "Compute 2467 + 3589. Do not use tools or run commands. Reply with only the integer."}],
            })
            first_text = "".join(filter(None, (_extract_text(n) for n in client.notifications[start:])))
            first_notifications = client.notifications[start:]
            history["first_prompt"] = {"rpc": _safe_rpc(first_rpc, redactor), "assistant_text": redactor.text(first_text), "duration_ms": first_ms}
            switch, switch_ms = client.rpc("session/set_config_option", {
                "sessionId": history_sid, "configId": "model", "value": history_target,
            })
            history["switch_rpc"] = {"rpc": _safe_rpc(switch, redactor), "duration_ms": switch_ms}
            start = len(client.notifications)
            second_rpc, second_ms = client.rpc("session/prompt", {
                "sessionId": history_sid,
                "prompt": [{"type": "text", "text": "Take the result from my previous message, multiply it by 3, then add 7. Do not use tools or run commands. Reply with only the integer."}],
            })
            second_text = "".join(filter(None, (_extract_text(n) for n in client.notifications[start:])))
            second_notifications = client.notifications[start:]
            history["second_prompt"] = {"rpc": _safe_rpc(second_rpc, redactor), "assistant_text": redactor.text(second_text), "duration_ms": second_ms}
            first_tool_calls = len(_tool_call_ids(first_notifications, history_sid))
            second_tool_calls = len(_tool_call_ids(second_notifications, history_sid))
            history["tool_call_counts"] = {"first_prompt": first_tool_calls, "second_prompt": second_tool_calls}
            history["checks"] = {
                "first_answer_is_6056": bool(re.search(r"\b6056\b", first_text)),
                "second_answer_is_18175": bool(re.search(r"\b18175\b", second_text)),
                "no_tool_calls": _execution_summary(executions_path)["total"] == executions_before_history
                    and first_tool_calls == 0 and second_tool_calls == 0,
            }
            history["status"] = "passed" if _result_ok(first_rpc) and _result_ok(switch) and _result_ok(second_rpc) and all(history["checks"].values()) else "failed"
        except (TimeoutError, ValidationError, OSError) as error:
            history["status"] = "failed"
            history["error"] = redactor.text(str(error))
        summary["history_session"] = history
        summary["execution_counts"] = _execution_summary(executions_path)

        # Keep only a compact permission/model-switch count in public output.
        summary["resident_process_pid"] = client.process.pid
        summary["rpc_method_counts"] = _rpc_counts(client.raw_file.name)
        summary["private_evidence"] = {
            "rpc_raw": str(run_dir / "rpc-raw.jsonl"),
            "stderr": str(run_dir / "stderr.log"),
            "grow_home": str(grow_home),
            "workspace": str(workspace),
            "raw_notifications_are_private": True,
        }
    except (RpcTimeout, ValidationError, OSError, TimeoutError) as error:
        summary["fatal_error"] = redactor.text(str(error))
    finally:
        if client is not None:
            try:
                client.close()
            except Exception as error:
                summary["cleanup_error"] = redactor.text(str(error))

    passed = sum(1 for stage in summary["stages"] if stage.get("status") == "passed")
    failed = sum(1 for stage in summary["stages"] if stage.get("status") not in {"passed", "not_run"})
    not_run = sum(1 for stage in summary["stages"] if stage.get("status") == "not_run")
    summary["matrix_counts"] = {"passed": passed, "failed": failed, "not_run": not_run,
                                 "total": len(summary["stages"])}
    summary = redactor.value(summary)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    # Refuse to overwrite an existing result; write the complete summary once.
    fd = os.open(output_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as stream:
        stream.write(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
        stream.flush()
        os.fsync(stream.fileno())
    if not args.keep_private_run:
        shutil.rmtree(run_dir)
    print("[4/4] complete", flush=True)
    print(json.dumps({
        "output": str(output_path),
        "private_run_dir": str(run_dir) if args.keep_private_run else None,
        "matrix_counts": summary["matrix_counts"],
        "history_status": (summary.get("history_session") or {}).get("status"),
        "result": "redacted",
    }, ensure_ascii=False), flush=True)
    return 0 if failed == 0 and not_run == 0 and (summary.get("history_session") or {}).get("status") == "passed" else 1


def _rpc_error_message(response: dict[str, Any], redactor: Redactor) -> str:
    error = response.get("error") or {}
    message = error.get("message") if isinstance(error, dict) else None
    return redactor.text(message) if isinstance(message, str) else "RPC returned an error"


def _execution_summary(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {"total": 0, "by_stage": {}}
    counts: dict[str, int] = {}
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            try:
                stage = str(json.loads(line).get("stage"))
            except json.JSONDecodeError:
                continue
            counts[stage] = counts.get(stage, 0) + 1
    return {"total": sum(counts.values()), "by_stage": counts}


def _rpc_counts(path: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    try:
        with open(path, encoding="utf-8") as stream:
            for line in stream:
                try:
                    message = json.loads(line)
                except json.JSONDecodeError:
                    continue
                method = (message.get("client") or {}).get("method")
                if isinstance(method, str):
                    counts[method] = counts.get(method, 0) + 1
    except OSError:
        pass
    return counts


RECORD_STEP_SCRIPT = '''#!/usr/bin/env python3
import json
import os
import secrets
import sys

if len(sys.argv) != 2 or not sys.argv[1].isdigit():
    raise SystemExit("usage: record_step.py <stage-number>")
stage = int(sys.argv[1])
token = secrets.token_urlsafe(24)
record = json.dumps({"stage": stage, "token": token}, separators=(",", ":")) + "\\n"
fd = os.open("executions.jsonl", os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
with os.fdopen(fd, "a", encoding="utf-8") as stream:
    stream.write(record)
    stream.flush()
    os.fsync(stream.fileno())
print(token)
'''


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="freshly built grow binary")
    parser.add_argument("--source-config", required=True, help="provider/model source TOML")
    parser.add_argument("--output", required=True, help="new, redacted JSON result path")
    parser.add_argument("--matrix", choices=tuple(ROUTE_MATRICES), default="primary")
    parser.add_argument("--history-source", help=f"history-check source model (default: {HISTORY_SOURCE})")
    parser.add_argument("--history-target", help="history-check target model (matrix-specific default)")
    parser.add_argument("--keep-private-run", action="store_true",
                        help="retain mode-700 temp run with raw RPC, stderr and isolated sessions")
    args = parser.parse_args()
    try:
        return run(args)
    except (OSError, tomllib.TOMLDecodeError, ValidationError) as error:
        # Errors raised before a private redactor is established deliberately
        # expose no exception details that could contain source configuration.
        print(f"validation driver failed: {type(error).__name__}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
