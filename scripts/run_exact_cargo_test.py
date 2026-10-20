#!/usr/bin/env python3
"""Run one Cargo test by exact name and prove that exactly one test ran."""

from __future__ import annotations

import argparse
import codecs
import math
import os
import queue
import re
import signal
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_COMPILE_TIMEOUT = 60 * 60
DEFAULT_LIST_TIMEOUT = 60
DEFAULT_TEST_TIMEOUT = 15 * 60
OUTPUT_TAIL_LIMIT = 64 * 1024
TERMINATE_GRACE_SECONDS = 0.5
READER_SHUTDOWN_GRACE_SECONDS = 1.0
LISTED_TEST = re.compile(r"^(.+): test(?: \(ignored:.*\))?$")
TEST_PASSED = re.compile(r"^test (.+) \.\.\. ok$")
TEST_SUMMARY = re.compile(
    r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
)


class TestSelectionError(RuntimeError):
    pass


class CancellationRequested(BaseException):
    pass


@dataclass
class CommandOutput:
    returncode: int = 0
    tail: str = ""
    listed_matches: int = 0
    passed_tests: list[str] = field(default_factory=list)
    summaries: list[tuple[int, int, int]] = field(default_factory=list)


def cargo_commands(package: str, target: str | None, test_name: str, ignored: bool):
    base = ["cargo", "test", "--locked", "-p", package]
    base.extend(["--lib"] if target is None else ["--test", target])
    build_command = [*base, "--no-run"]
    list_command = [*base, test_name, "--", "--exact", "--list"]
    run_arguments = ["--exact", "--format=pretty", "--color=never", "--test-threads=1"]
    if ignored:
        run_arguments.append("--ignored")
    run_command = [*base, test_name, "--", *run_arguments]
    return build_command, list_command, run_command


def _terminate_process_tree(process: subprocess.Popen[bytes]) -> bool:
    """Attempt to stop the command tree and reap the direct child within a bound."""
    tree_signal_sent = False
    if os.name == "nt":
        try:
            result = subprocess.run(
                ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=3,
            )
            tree_signal_sent = result.returncode == 0
        except (OSError, subprocess.TimeoutExpired):
            tree_signal_sent = False
        if process.poll() is None:
            process.kill()
        try:
            process.wait(timeout=TERMINATE_GRACE_SECONDS)
        except subprocess.TimeoutExpired:
            process.kill()
            try:
                process.wait(timeout=TERMINATE_GRACE_SECONDS)
            except subprocess.TimeoutExpired:
                return False
        return tree_signal_sent and process.poll() is not None

    try:
        os.killpg(process.pid, signal.SIGTERM)
        tree_signal_sent = True
    except (ProcessLookupError, PermissionError):
        pass
    try:
        process.wait(timeout=TERMINATE_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        pass
    # Kill the group even if its leader exited: descendants may still be alive.
    try:
        os.killpg(process.pid, signal.SIGKILL)
        tree_signal_sent = True
    except (ProcessLookupError, PermissionError):
        pass
    if process.poll() is None:
        try:
            process.wait(timeout=TERMINATE_GRACE_SECONDS)
        except subprocess.TimeoutExpired:
            process.kill()
            try:
                process.wait(timeout=TERMINATE_GRACE_SECONDS)
            except subprocess.TimeoutExpired:
                return False
    return tree_signal_sent and process.poll() is not None


def _append_tail(output: CommandOutput, text: str) -> None:
    output.tail = (output.tail + text)[-OUTPUT_TAIL_LIMIT:]


def _read_output_chunk(stdout) -> bytes:
    return os.read(stdout.fileno(), 4096)


def _parse_line(output: CommandOutput, line: str, selected_test: str | None) -> None:
    listed = LISTED_TEST.match(line)
    if listed and (selected_test is None or listed.group(1) == selected_test):
        output.listed_matches += 1
    passed = TEST_PASSED.match(line)
    if passed:
        output.passed_tests.append(passed.group(1))
    summary = TEST_SUMMARY.match(line)
    if summary:
        output.summaries.append(tuple(map(int, summary.groups())))


def _run_phase(
    command: list[str], phase: str, timeout: float, *, selected_test: str | None = None
) -> CommandOutput:
    print(f"[{phase}] starting (timeout: {timeout:g}s)", file=sys.stderr, flush=True)
    creation_options: dict[str, object] = {}
    if os.name == "nt":
        creation_options["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        creation_options["start_new_session"] = True
    process = subprocess.Popen(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        bufsize=0,
        **creation_options,
    )
    assert process.stdout is not None
    chunks: queue.Queue[bytes | None] = queue.Queue(maxsize=64)
    reader_done = threading.Event()
    stop_queuing = threading.Event()
    reader_errors: list[OSError | ValueError] = []

    def enqueue(chunk: bytes | None) -> None:
        while not stop_queuing.is_set():
            try:
                chunks.put(chunk, timeout=0.05)
                return
            except queue.Full:
                continue

    def read_output() -> None:
        try:
            while True:
                chunk = _read_output_chunk(process.stdout)
                if not chunk:
                    break
                enqueue(chunk)
        except (OSError, ValueError) as error:
            reader_errors.append(error)
        finally:
            reader_done.set()
            enqueue(None)
            try:
                process.stdout.close()
            except (OSError, ValueError):
                pass

    reader = threading.Thread(target=read_output, name=f"{phase}-output", daemon=True)
    reader.start()
    result = CommandOutput()
    decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
    pending_line = ""
    deadline = time.monotonic() + timeout
    eof = False
    failure: TestSelectionError | None = None
    cancellation: BaseException | None = None
    tree_cleanup_confirmed = True
    try:
        while not eof or process.poll() is None:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                tree_cleanup_confirmed = _terminate_process_tree(process)
                failure = TestSelectionError(
                    f"Cargo {phase} timed out after {timeout:g}s:\n{result.tail}"
                )
                break
            try:
                chunk = chunks.get(timeout=min(remaining, 0.1))
            except queue.Empty:
                continue
            if chunk is None:
                eof = True
                text = decoder.decode(b"", final=True)
            else:
                text = decoder.decode(chunk)
            if text:
                sys.stdout.write(text)
                sys.stdout.flush()
                _append_tail(result, text)
                pending_line += text
                lines = pending_line.split("\n")
                pending_line = lines.pop()
                for line in lines:
                    _parse_line(result, line.rstrip("\r"), selected_test)
                if len(pending_line) > OUTPUT_TAIL_LIMIT:
                    pending_line = pending_line[-OUTPUT_TAIL_LIMIT:]
        if pending_line:
            _parse_line(result, pending_line.rstrip("\r"), selected_test)
        if failure is None:
            result.returncode = process.wait(timeout=max(0.0, deadline - time.monotonic()))
    except (KeyboardInterrupt, CancellationRequested) as error:
        cancellation = error
        tree_cleanup_confirmed = _terminate_process_tree(process)
    finally:
        if process.poll() is None:
            tree_cleanup_confirmed = _terminate_process_tree(process) and tree_cleanup_confirmed
        reader_deadline = time.monotonic() + READER_SHUTDOWN_GRACE_SECONDS
        while reader.is_alive() and time.monotonic() < reader_deadline:
            try:
                chunks.get(timeout=min(0.05, max(0.0, reader_deadline - time.monotonic())))
            except queue.Empty:
                pass
        if reader.is_alive():
            stop_queuing.set()
        reader.join(timeout=0)
        if not reader.is_alive():
            try:
                process.stdout.close()
            except (OSError, ValueError):
                pass

    cleanup_confirmed = (
        tree_cleanup_confirmed
        and process.poll() is not None
        and reader_done.is_set()
        and not reader.is_alive()
    )
    if cancellation is not None:
        if reader_errors:
            print(f"[{phase}] output reader failed: {reader_errors[0]}", file=sys.stderr, flush=True)
        if cleanup_confirmed:
            print(f"[{phase}] cancelled; process tree termination confirmed", file=sys.stderr, flush=True)
        else:
            print(
                f"[{phase}] cancelled; process-tree cleanup unconfirmed (a descendant may still be running)",
                file=sys.stderr,
                flush=True,
            )
        raise cancellation
    if failure is not None:
        if reader_errors:
            failure.args = (f"{failure}; output reader failed: {reader_errors[0]}",)
        if not cleanup_confirmed:
            failure.args = (
                f"{failure}; process-tree cleanup unconfirmed (a descendant may still be running)",
            )
        raise failure
    if not cleanup_confirmed:
        raise TestSelectionError(
            f"Cargo {phase} ended without confirming process-tree cleanup (a descendant may still be running)"
        )
    if reader_errors:
        raise TestSelectionError(
            f"Cargo {phase} output reader failed: {reader_errors[0]}\n{result.tail}"
        )
    print(f"[{phase}] finished (exit {result.returncode})", file=sys.stderr, flush=True)
    return result


def execute(
    package: str,
    target: str | None,
    test_name: str,
    ignored: bool = False,
    *,
    compile_timeout: float = DEFAULT_COMPILE_TIMEOUT,
    list_timeout: float = DEFAULT_LIST_TIMEOUT,
    test_timeout: float = DEFAULT_TEST_TIMEOUT,
) -> None:
    build_command, list_command, run_command = cargo_commands(package, target, test_name, ignored)
    try:
        built = _run_phase(build_command, "compile", compile_timeout)
    except TestSelectionError as error:
        if "timed out" in str(error):
            raise
        raise TestSelectionError(f"Cargo compilation failed:\n{error}") from error
    if built.returncode:
        raise TestSelectionError(f"Cargo compilation failed (exit {built.returncode}):\n{built.tail}")

    try:
        listed = _run_phase(
            list_command, "enumerate", list_timeout, selected_test=test_name
        )
    except TestSelectionError as error:
        if "timed out" in str(error):
            raise
        raise TestSelectionError(
            f"Cargo could not enumerate {package}:{target or 'lib'}::{test_name}:\n{error}"
        ) from error
    if listed.returncode:
        raise TestSelectionError(
            f"Cargo could not enumerate {package}:{target or 'lib'}::{test_name} "
            f"(exit {listed.returncode}):\n{listed.tail}"
        )

    if listed.listed_matches != 1:
        raise TestSelectionError(
            f"expected exactly one listed test named {test_name!r}, found {listed.listed_matches}"
        )

    result = _run_phase(run_command, "execute", test_timeout)
    if result.returncode:
        raise TestSelectionError(
            f"Cargo test failed for {test_name!r} (exit {result.returncode}):\n{result.tail}"
        )
    summaries = result.summaries
    if test_name not in result.passed_tests or not summaries or summaries[-1] != (1, 0, 0):
        raise TestSelectionError(
            f"expected Cargo to report exactly one passed, non-ignored test named {test_name!r}; "
            f"passed names {result.passed_tests or 'none'}, final summary "
            f"{summaries[-1] if summaries else 'missing'}:\n{result.tail}"
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", required=True, help="Cargo package name")
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--lib", action="store_true", help="select the package library")
    selection.add_argument("--test", dest="target", help="select an integration-test target")
    parser.add_argument("--ignored", action="store_true", help="run the exact ignored test")
    parser.add_argument(
        "--compile-timeout", type=float, default=DEFAULT_COMPILE_TIMEOUT,
        help=f"compilation deadline in seconds (default: {DEFAULT_COMPILE_TIMEOUT})",
    )
    parser.add_argument(
        "--list-timeout", type=float, default=DEFAULT_LIST_TIMEOUT,
        help=f"test enumeration deadline in seconds (default: {DEFAULT_LIST_TIMEOUT})",
    )
    parser.add_argument(
        "--test-timeout", type=float, default=DEFAULT_TEST_TIMEOUT,
        help=f"test execution deadline in seconds (default: {DEFAULT_TEST_TIMEOUT})",
    )
    parser.add_argument("test_name", help="fully-qualified Rust test name")
    arguments = parser.parse_args(argv)
    for name in ("compile_timeout", "list_timeout", "test_timeout"):
        value = getattr(arguments, name)
        if not math.isfinite(value) or value <= 0:
            parser.error(f"--{name.replace('_', '-')} must be finite and greater than zero")

    previous_sigterm_handler = signal.getsignal(signal.SIGTERM)
    cancellation_exit_code = 130

    def cancel_on_sigterm(_signum, _frame) -> None:
        nonlocal cancellation_exit_code
        cancellation_exit_code = 143
        raise CancellationRequested()

    signal.signal(signal.SIGTERM, cancel_on_sigterm)
    try:
        execute(
            arguments.package,
            arguments.target,
            arguments.test_name,
            arguments.ignored,
            compile_timeout=arguments.compile_timeout,
            list_timeout=arguments.list_timeout,
            test_timeout=arguments.test_timeout,
        )
    except (KeyboardInterrupt, CancellationRequested):
        print("Cancelled exact Cargo test run", file=sys.stderr)
        return cancellation_exit_code
    except TestSelectionError as error:
        print(error, file=sys.stderr)
        return 1
    finally:
        signal.signal(signal.SIGTERM, previous_sigterm_handler)
    print(f"Passed exactly one test: {arguments.test_name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
