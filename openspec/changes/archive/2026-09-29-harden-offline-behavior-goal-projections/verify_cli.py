"""Offline Behavior/Goal and interruption smoke: python3 verify_cli.py /absolute/path/to/grow.

Uses a disposable Grow home and a real PTY. No provider or running agent needed.
"""

import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
from urllib.parse import quote


def hashes(directory):
    return {
        str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in directory.rglob("*")
        if path.is_file()
    }


def main(binary):
    with tempfile.TemporaryDirectory(prefix="grow-transcript-adversarial-") as tmp:
        root = Path(tmp)
        home = root / "grow-home"
        session = home / "sessions" / quote(str(root), safe="") / "audit-session"
        session.mkdir(parents=True)
        summary = {
            "info": {"id": "audit-session", "cwd": str(root)},
            "created_at": "2026-09-29T00:00:00Z",
            "updated_at": "2026-09-29T00:00:00Z",
            "num_messages": 9,
            "current_model_id": "model",
            "session_format_version": 6,
        }
        (session / "summary.json").write_text(json.dumps(summary))
        (session / "timeline.jsonl").write_text("")
        sentinel = root / "MUST_NOT_EXECUTE"
        records = []

        def record(update, prompt, index, grow=False):
            records.append({
                "timestamp": 1790640000,
                "method": "_grow/session/update" if grow else "session/update",
                "params": {
                    "sessionId": "audit-session", "update": update,
                    "_meta": {"promptId": prompt, "agentTimestampMs": 1790640000000 + index * 100},
                },
            })

        def chunk(kind, text, prompt, index):
            update = {"sessionUpdate": kind, "content": {"type": "text", "text": text}}
            if kind == "user_message_chunk":
                update["_meta"] = {"messageId": prompt, "promptIndex": index}
            record(update, prompt, index)

        def mode(value, phase=None):
            record({"sessionUpdate": "current_mode_update", "currentModeId": value,
                    "_meta": {"grow/planPhase": phase}}, "p1", len(records))

        def goal(status, goal_id="goal-1"):
            record({"sessionUpdate": "goal_updated", "goal_id": goal_id,
                    "objective": "ship safely", "status": status, "tokens_used": 12,
                    "token_budget": None, "elapsed_ms": 1500,
                    "created_at": "2026-09-29T00:00:00Z",
                    "updated_at": "2026-09-29T00:00:01Z"}, "p1", len(records), True)

        mode("normal")
        chunk("user_message_chunk", "first user request", "p1", 0)
        chunk("agent_message_chunk", "interrupted visible response", "p1", 1)
        mode("goal")
        goal("active")
        record({"sessionUpdate": "tool_call", "toolCallId": "tool-1", "title": "sentinel",
                "kind": "execute", "status": "pending", "rawInput": {"command": f"touch {sentinel}"}}, "p1", 2)
        record({"sessionUpdate": "turn_completed", "prompt_id": "p1", "stop_reason": "cancelled"}, "p1", 3, True)
        goal("paused")
        mode("normal")
        goal("cleared", "")
        goal("active")  # Late old identity must not resurrect a cleared Goal.
        mode("plan", "awaiting_approval")
        mode("normal")
        mode("ask")  # Canonical wire ID of Clarify.
        mode("workflow")
        mode("normal")
        chunk("user_message_chunk", "second user request", "p2", 4)
        chunk("agent_message_chunk", "continued response A", "p2", 5)
        record({"sessionUpdate": "turn_completed", "prompt_id": "p1", "stop_reason": "cancelled"}, "p1", 6, True)
        chunk("agent_message_chunk", " B", "p2", 7)
        record({"sessionUpdate": "turn_completed", "prompt_id": "p2", "stop_reason": "end_turn"}, "p2", 8, True)
        (session / "updates.jsonl").write_text("".join(json.dumps(row) + "\n" for row in records))
        before = hashes(home / "sessions")
        env = dict(os.environ, GROW_HOME=str(home), TERM="xterm-256color")
        result = subprocess.run([binary, "export", "audit-session"], cwd=root, env=env,
                                capture_output=True, text=True, timeout=20)
        assert result.returncode == 0, result.stderr
        transcript = root / "audit-session" / "transcript.md"
        body = transcript.read_text()
        for expected in ["first user request", "second user request", "continued response A B",
                         "Turn p1 · cancelled", "still running at the captured snapshot"]:
            assert expected in body, (expected, body)
        assert body.count("Turn p1 · cancelled") == 1
        for behavior in ["normal", "goal", "plan / awaiting_approval", "ask", "workflow"]:
            assert f"Behavior · {behavior}" in body, body
        assert "Goal · ship safely" not in body, body
        # Existing output must survive a second publication attempt.
        published = transcript.read_bytes()
        result = subprocess.run([binary, "export", "audit-session"], cwd=root, env=env,
                                capture_output=True, timeout=20)
        assert result.returncode != 0
        assert transcript.read_bytes() == published

        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        original_termios = termios.tcgetattr(slave)
        process = subprocess.Popen([binary, "replay", "audit-session", "--speed", "100"],
                                   cwd=root, env=env, stdin=slave, stdout=slave, stderr=slave)
        output = bytearray()

        def drain(seconds):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                ready, _, _ = select.select([master], [], [], min(0.05, deadline - time.monotonic()))
                if ready:
                    output.extend(os.read(master, 65536))

        try:
            drain(2)
            assert process.poll() is None, output.decode(errors="replace")
            readable = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b" ", output)
            assert re.search(rb"Behavior:\s+normal", readable), output.decode(errors="replace")
            assert b"\x1b[?2004h" in output, "bracketed paste was not enabled"
            assert b"Finished" in output or b"finished" in output, output.decode(errors="replace")
            os.write(master, b"\x1b[200~q /help + - \x1b[201~")
            drain(0.3)
            assert process.poll() is None, "pasted q incorrectly exited replay"
            os.write(master, b"q")
            process.wait(timeout=5)
            drain(0.1)
            assert process.returncode == 0
            assert termios.tcgetattr(slave) == original_termios, "terminal modes not restored"
            assert b"\x1b[?2004l" in output and b"\x1b[?1049l" in output
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)
        assert hashes(home / "sessions") == before, "offline command changed session files"
        assert not sentinel.exists(), "a recorded tool was executed"
        print("PASS: Behavior changes, Goal clear and stale update, Plan phase, cancelled/continued export, late terminal dedup, pending tool, no-replace publication,")
        print("      real replay PTY, paste isolation, finished browsing, terminal restoration,")
        print("      unchanged session bytes and no recorded command execution")


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve()))
