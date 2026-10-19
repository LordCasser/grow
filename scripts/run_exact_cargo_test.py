#!/usr/bin/env python3
"""Run one Cargo test by exact name and prove that exactly one test ran."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
LISTED_TEST = re.compile(r"^(.+): test(?: \(ignored:.*\))?$", re.MULTILINE)
TEST_PASSED = re.compile(r"^test (.+) \.\.\. ok$", re.MULTILINE)
TEST_SUMMARY = re.compile(
    r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;",
    re.MULTILINE,
)


class TestSelectionError(RuntimeError):
    pass


def cargo_commands(package: str, target: str | None, test_name: str, ignored: bool):
    base = ["cargo", "test", "--locked", "-p", package]
    base.extend(["--lib"] if target is None else ["--test", target])
    base.append(test_name)

    list_command = [*base, "--", "--exact", "--list"]
    # Keep successful test stdout captured: --nocapture can interleave
    # application output with libtest's named result line.
    run_arguments = ["--exact", "--format=pretty", "--color=never", "--test-threads=1"]
    if ignored:
        run_arguments.append("--ignored")
    run_command = [*base, "--", *run_arguments]
    return list_command, run_command


def execute(package: str, target: str | None, test_name: str, ignored: bool = False) -> None:
    list_command, run_command = cargo_commands(package, target, test_name, ignored)
    listed = subprocess.run(
        list_command,
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if listed.returncode:
        raise TestSelectionError(
            f"Cargo could not list {package}:{target or 'lib'}::{test_name}:\n{listed.stdout}"
        )

    matches = [name for name in LISTED_TEST.findall(listed.stdout) if name == test_name]
    if len(matches) != 1:
        raise TestSelectionError(
            f"expected exactly one listed test named {test_name!r}, found {len(matches)}"
        )

    result = subprocess.run(
        run_command,
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if result.returncode:
        raise TestSelectionError(
            f"Cargo test failed for {test_name!r} (exit {result.returncode}):\n{result.stdout}"
        )

    passed_tests = TEST_PASSED.findall(result.stdout)
    summaries = [tuple(map(int, counts)) for counts in TEST_SUMMARY.findall(result.stdout)]
    if test_name not in passed_tests or not summaries or summaries[-1] != (1, 0, 0):
        raise TestSelectionError(
            f"expected Cargo to report exactly one passed, non-ignored test named {test_name!r}; "
            f"passed names {passed_tests or 'none'}, final summary "
            f"{summaries[-1] if summaries else 'missing'}:\n{result.stdout}"
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", required=True, help="Cargo package name")
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--lib", action="store_true", help="select the package library")
    selection.add_argument("--test", dest="target", help="select an integration-test target")
    parser.add_argument("--ignored", action="store_true", help="run the exact ignored test")
    parser.add_argument("test_name", help="fully-qualified Rust test name")
    arguments = parser.parse_args(argv)

    try:
        execute(arguments.package, arguments.target, arguments.test_name, arguments.ignored)
    except TestSelectionError as error:
        print(error, file=sys.stderr)
        return 1
    print(f"Passed exactly one test: {arguments.test_name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
