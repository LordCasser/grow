from __future__ import annotations

import contextlib
import io
import json
import _thread
import os
import signal
import subprocess
import sys
import tempfile
import threading
import textwrap
import time
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import run_exact_cargo_test


def phase_output(
    returncode: int = 0,
    *,
    listed: list[str] | None = None,
    passed: list[str] | None = None,
    summaries: list[tuple[int, int, int]] | None = None,
    tail: str = "",
    test_executables: list[str] | None = None,
    package_directory: str = "/tmp/fixture-package",
) -> run_exact_cargo_test.CommandOutput:
    return run_exact_cargo_test.CommandOutput(
        returncode=returncode,
        tail=tail,
        listed_matches=len(listed or []),
        passed_tests=passed or [],
        summaries=summaries or [],
        test_executables={
            executable: package_directory
            for executable in (
                ["/tmp/fixture-test-binary"] if test_executables is None else test_executables
            )
        },
    )


def stop_fixture_process(pid: int) -> None:
    try:
        os.kill(pid, getattr(signal, "SIGKILL", signal.SIGTERM))
    except OSError:
        pass


class ExactCargoTestTests(unittest.TestCase):
    def invoke(self, ignored: bool = False) -> None:
        run_exact_cargo_test.execute(
            "pager", "pty_e2e_persistence", "module::scenario", ignored
        )

    def outcomes(self, listed: list[str] | None = None, execution: run_exact_cargo_test.CommandOutput | None = None):
        return [
            phase_output(),
            phase_output(listed=listed or ["module::scenario"]),
            execution or phase_output(passed=["module::scenario"], summaries=[(1, 0, 0)]),
        ]

    def test_compiles_enumerates_and_runs_with_ignored_opt_in(self) -> None:
        with patch.object(run_exact_cargo_test, "_run_phase", side_effect=self.outcomes()) as run:
            self.invoke(ignored=True)
        self.assertEqual(run.call_count, 3)
        compile_command = run.call_args_list[0].args[0]
        list_command = run.call_args_list[1].args[0]
        execute_command = run.call_args_list[2].args[0]
        self.assertEqual(compile_command[0], "cargo")
        self.assertIn("--no-run", compile_command)
        self.assertIn("--message-format=json", compile_command)
        self.assertEqual(list_command[0], "/tmp/fixture-test-binary")
        self.assertIn("--list", list_command)
        self.assertEqual(run.call_args_list[1].kwargs["cwd"], "/tmp/fixture-package")
        self.assertEqual(execute_command[0], list_command[0])
        self.assertEqual(run.call_args_list[2].kwargs["cwd"], "/tmp/fixture-package")
        self.assertIn("--ignored", execute_command)
        self.assertEqual([call.args[1] for call in run.call_args_list], ["compile", "enumerate", "execute"])

    def test_missing_and_ambiguous_names_fail_before_execution(self) -> None:
        for names, expected in [([], "found 0"), (["module::scenario", "module::scenario"], "found 2")]:
            with self.subTest(names=names), patch.object(
                run_exact_cargo_test, "_run_phase", side_effect=[phase_output(), phase_output(listed=names)]
            ) as run:
                with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, expected):
                    self.invoke()
                self.assertEqual(run.call_count, 2)

    def test_missing_or_multiple_test_artifacts_fail_closed(self) -> None:
        for executables, expected in [([], "found 0"), (["/tmp/one", "/tmp/two"], "found 2")]:
            with self.subTest(executables=executables), patch.object(
                run_exact_cargo_test,
                "_run_phase",
                return_value=phase_output(test_executables=executables),
            ) as run:
                with self.assertRaisesRegex(
                    run_exact_cargo_test.TestSelectionError,
                    f"exactly one test executable; {expected}",
                ):
                    self.invoke()
                self.assertEqual(run.call_count, 1)

    def test_compile_artifact_capture_is_not_limited_by_diagnostic_tail(self) -> None:
        output = run_exact_cargo_test.CommandOutput()
        message = {
            "reason": "compiler-artifact",
            "profile": {"test": True},
            "executable": "/tmp/long-artifact-test",
            "manifest_path": "/tmp/fixture-package/Cargo.toml",
            "large-metadata": "x" * (run_exact_cargo_test.OUTPUT_TAIL_LIMIT + 1),
        }
        rendered = run_exact_cargo_test._compile_line_output(output, json.dumps(message))
        self.assertEqual(rendered, "")
        self.assertEqual(
            output.test_executables,
            {"/tmp/long-artifact-test": str(Path("/tmp/fixture-package"))},
        )
        self.assertEqual(output.tail, "")

    def test_compile_messages_keep_rendered_or_raw_diagnostics(self) -> None:
        output = run_exact_cargo_test.CommandOutput()
        rendered = run_exact_cargo_test._compile_line_output(
            output,
            json.dumps({"reason": "compiler-message", "message": {"rendered": "warning: keep me\n"}}),
        )
        self.assertEqual(rendered, "warning: keep me\n")
        raw = json.dumps({"reason": "compiler-message", "message": {"message": "fallback"}})
        self.assertEqual(run_exact_cargo_test._compile_line_output(output, raw), raw + "\n")

    def test_compile_and_enumeration_failures_are_distinguished(self) -> None:
        with patch.object(
            run_exact_cargo_test, "_run_phase", return_value=phase_output(101, tail="compiler error")
        ):
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "compilation failed"):
                self.invoke()
        with patch.object(
            run_exact_cargo_test,
            "_run_phase",
            side_effect=[phase_output(), phase_output(101, tail="enumeration error")],
        ):
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "could not enumerate"):
                self.invoke()

    def test_zero_execution_and_ignored_pass_are_not_green(self) -> None:
        for execution in [
            phase_output(passed=[], summaries=[(0, 0, 0)]),
            phase_output(summaries=[(0, 0, 1)]),
        ]:
            with self.subTest(execution=execution), patch.object(
                run_exact_cargo_test, "_run_phase", side_effect=self.outcomes(execution=execution)
            ):
                with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "non-ignored test"):
                    self.invoke()

    def test_failing_test_is_not_hidden_by_reported_run_count(self) -> None:
        with patch.object(
            run_exact_cargo_test,
            "_run_phase",
            side_effect=self.outcomes(execution=phase_output(101, tail="test failed")),
        ):
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "exit 101"):
                self.invoke()

    def test_phase_deadlines_must_be_finite_and_positive(self) -> None:
        for flag in ("--compile-timeout", "--list-timeout", "--test-timeout"):
            for value in ("nan", "inf", "-inf", "0", "-1"):
                with self.subTest(flag=flag, value=value), contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit) as error:
                        run_exact_cargo_test.main(
                            ["--package", "fixture", "--lib", flag, value, "selected"]
                        )
                    self.assertEqual(error.exception.code, 2)

    def test_default_compile_budget_leaves_time_within_ninety_minute_gate(self) -> None:
        self.assertEqual(run_exact_cargo_test.DEFAULT_COMPILE_TIMEOUT, 60 * 60)
        self.assertLess(
            run_exact_cargo_test.DEFAULT_COMPILE_TIMEOUT
            + run_exact_cargo_test.DEFAULT_TEST_TIMEOUT
            + run_exact_cargo_test.DEFAULT_LIST_TIMEOUT,
            90 * 60,
        )


class RealCommandTests(unittest.TestCase):
    def test_output_read_failure_is_not_treated_as_clean_eof(self) -> None:
        with patch.object(
            run_exact_cargo_test,
            "_read_output_chunk",
            side_effect=OSError("injected read failure"),
        ):
            with self.assertRaisesRegex(
                run_exact_cargo_test.TestSelectionError,
                "output reader failed: injected read failure",
            ):
                run_exact_cargo_test._run_phase(
                    [sys.executable, "-c", "pass"], "read-failure", 5
                )

    def test_command_output_streams_before_process_exit(self) -> None:
        class ObservedOutput(io.StringIO):
            event = None

            def write(self, value: str) -> int:
                written = super().write(value)
                if "visible-now" in value:
                    self.event.set()
                return written

        observed = ObservedOutput()
        observed.event = threading.Event()
        failure: list[BaseException] = []
        with tempfile.TemporaryDirectory(prefix="grow-exact-stream-") as directory:
            release = Path(directory) / "release"
            code = (
                "import pathlib, time; print('visible-now', flush=True); "
                f"release=pathlib.Path({str(release)!r}); "
                "exec('while not release.exists():\\n time.sleep(0.01)')"
            )
            with contextlib.redirect_stdout(observed):
                worker = threading.Thread(
                    target=lambda: self._capture_failure(
                        failure,
                        lambda: run_exact_cargo_test._run_phase(
                            [sys.executable, "-u", "-c", code], "stream-test", 10
                        ),
                    )
                )
                worker.start()
                try:
                    self.assertTrue(observed.event.wait(5), "subprocess output was not streamed")
                    self.assertTrue(worker.is_alive(), "subprocess exited before receiving release")
                finally:
                    release.touch()
                    worker.join(5)
        self.assertFalse(failure)
        self.assertIn("visible-now", observed.getvalue())

    @staticmethod
    def _capture_failure(errors: list[BaseException], callback) -> None:
        try:
            callback()
        except BaseException as error:
            errors.append(error)

    def test_timeout_kills_real_descendant_process(self) -> None:
        with tempfile.TemporaryDirectory(prefix="grow-exact-timeout-") as directory:
            marker = Path(directory) / "descendant-survived"
            pid_path = Path(directory) / "descendant.pid"
            child = (
                "import time, pathlib; time.sleep(3); "
                f"pathlib.Path({str(marker)!r}).write_text('alive')"
            )
            parent = (
                "import subprocess, sys, time, pathlib; "
                f"p=subprocess.Popen([sys.executable, '-c', {child!r}]); "
                f"pathlib.Path({str(pid_path)!r}).write_text(str(p.pid)); time.sleep(10)"
            )
            try:
                with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "execute timed out"):
                    run_exact_cargo_test._run_phase(
                        [sys.executable, "-u", "-c", parent], "execute", 2
                    )
                self.assertTrue(pid_path.exists())
                time.sleep(1.2)
                self.assertFalse(marker.exists(), "grandchild continued after timed-out process tree cleanup")
            finally:
                if pid_path.exists():
                    stop_fixture_process(int(pid_path.read_text()))

    @unittest.skipIf(os.name == "nt", "Unix session escape reproduces a detached descendant")
    def test_timeout_is_bounded_when_exited_parent_leaves_inherited_pipe_open(self) -> None:
        with tempfile.TemporaryDirectory(prefix="grow-exact-orphan-pipe-") as directory:
            marker = Path(directory) / "descendant-survived"
            pid_path = Path(directory) / "descendant.pid"
            child = (
                "import time, pathlib; time.sleep(60); "
                f"pathlib.Path({str(marker)!r}).write_text('alive')"
            )
            parent = (
                "import subprocess, sys, pathlib; "
                f"p=subprocess.Popen([sys.executable, '-c', {child!r}], start_new_session=True); "
                f"pathlib.Path({str(pid_path)!r}).write_text(str(p.pid))"
            )
            try:
                started = time.monotonic()
                with self.assertRaisesRegex(
                    run_exact_cargo_test.TestSelectionError, "cleanup unconfirmed"
                ):
                    run_exact_cargo_test._run_phase(
                        [sys.executable, "-u", "-c", parent], "execute", 2
                    )
                self.assertLess(time.monotonic() - started, 5.5)
                self.assertTrue(pid_path.exists())
            finally:
                if pid_path.exists():
                    stop_fixture_process(int(pid_path.read_text()))
            time.sleep(0.1)
            self.assertFalse(marker.exists(), "test cleanup left the detached fixture running")

    def test_interrupt_kills_real_descendant_process(self) -> None:
        with tempfile.TemporaryDirectory(prefix="grow-exact-cancel-") as directory:
            marker = Path(directory) / "descendant-survived"
            pid_path = Path(directory) / "descendant.pid"
            child = (
                "import time, pathlib; time.sleep(1.5); "
                f"pathlib.Path({str(marker)!r}).write_text('alive')"
            )
            parent = (
                "import subprocess, sys, time, pathlib; "
                f"p=subprocess.Popen([sys.executable, '-c', {child!r}]); "
                f"pathlib.Path({str(pid_path)!r}).write_text(str(p.pid)); time.sleep(10)"
            )
            interrupt_sent = threading.Event()

            def interrupt_when_child_is_ready() -> None:
                deadline = time.monotonic() + 5
                while not pid_path.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                if pid_path.exists():
                    interrupt_sent.set()
                    _thread.interrupt_main()

            interrupter = threading.Thread(target=interrupt_when_child_is_ready, daemon=True)
            interrupter.start()
            with self.assertRaises(KeyboardInterrupt):
                run_exact_cargo_test._run_phase(
                    [sys.executable, "-u", "-c", parent], "execute", 10
                )
            interrupter.join(timeout=1)
            self.assertTrue(interrupt_sent.is_set(), "runner did not start the real child process tree")
            time.sleep(1.7)
            self.assertFalse(marker.exists(), "grandchild continued after cancellation cleanup")

    @unittest.skipIf(os.name == "nt", "SIGTERM signal delivery is Unix-specific")
    def test_sigterm_kills_real_descendant_process(self) -> None:
        with tempfile.TemporaryDirectory(prefix="grow-exact-sigterm-") as directory:
            marker = Path(directory) / "descendant-survived"
            pid_path = Path(directory) / "descendant.pid"
            child = (
                "import time, pathlib; time.sleep(1.5); "
                f"pathlib.Path({str(marker)!r}).write_text('alive')"
            )
            parent = (
                "import subprocess, sys, time, pathlib; "
                f"p=subprocess.Popen([sys.executable, '-c', {child!r}]); "
                f"pathlib.Path({str(pid_path)!r}).write_text(str(p.pid)); time.sleep(10)"
            )
            module_path = str(Path(run_exact_cargo_test.__file__).resolve())
            wrapper = textwrap.dedent(
                f"""
                import importlib.util, sys
                spec = importlib.util.spec_from_file_location('exact_runner', {module_path!r})
                runner = importlib.util.module_from_spec(spec)
                sys.modules[spec.name] = runner
                spec.loader.exec_module(runner)
                runner.execute = lambda *args, **kwargs: runner._run_phase(
                    [sys.executable, '-u', '-c', {parent!r}], 'execute', 10)
                raise SystemExit(runner.main(['--package', 'fixture', '--lib', 'selected']))
                """
            )
            process = subprocess.Popen(
                [sys.executable, "-u", "-c", wrapper], stdout=subprocess.PIPE, stderr=subprocess.PIPE
            )
            try:
                deadline = time.monotonic() + 5
                while not pid_path.exists() and process.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.02)
                if not pid_path.exists():
                    process.terminate()
                    stdout, stderr = process.communicate(timeout=5)
                    self.fail(
                        "runner did not start the real child process tree; "
                        f"exit_code={process.returncode}, "
                        f"stdout={stdout.decode(errors='replace')}, "
                        f"stderr={stderr.decode(errors='replace')}"
                    )
                process.send_signal(signal.SIGTERM)
                process.communicate(timeout=5)
                self.assertEqual(process.returncode, 143)
                time.sleep(1.7)
                self.assertFalse(marker.exists(), "grandchild continued after SIGTERM cleanup")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()

    def test_windows_tree_cleanup_uses_builtin_taskkill(self) -> None:
        class FakeProcess:
            pid = 456

            def poll(self):
                return 0

            def kill(self):
                pass

            def wait(self, timeout=None):
                return 0

        with patch.object(run_exact_cargo_test.os, "name", "nt"), patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            return_value=type("Result", (), {"returncode": 0})(),
        ) as run:
            self.assertTrue(run_exact_cargo_test._terminate_process_tree(FakeProcess()))  # type: ignore[arg-type]
        self.assertEqual(run.call_args.args[0], ["taskkill", "/PID", "456", "/T", "/F"])
        self.assertTrue(run.call_args.kwargs["check"] is False)


class RealLibtestSelectionTests(unittest.TestCase):
    """Exercise artifact selection and result parsing against Rust's real test harness."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.directory = tempfile.TemporaryDirectory(prefix="grow-exact-test-")
        cls.root = Path(cls.directory.name)
        cls.package_root = cls.root / "fixture"
        cls.package_root.mkdir()
        (cls.root / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["fixture"]\nresolver = "2"\n'
        )
        (cls.package_root / "Cargo.toml").write_text(
            '[package]\nname = "grow-exact-fixture"\nversion = "0.0.0"\nedition = "2021"\nbuild = "build.rs"\n'
        )
        (cls.package_root / "build.rs").write_text(
            "use std::{env, fs::OpenOptions, io::Write, thread, time::Duration};\n"
            "fn main() {\n"
            "  println!(\"cargo:rerun-if-changed=build.rs\");\n"
            "  println!(\"cargo:rerun-if-changed=rerun-trigger\");\n"
            "  println!(\"cargo:rerun-if-env-changed=GROW_EXACT_BUILD_INVOCATIONS\");\n"
            "  if let Ok(path) = env::var(\"GROW_EXACT_BUILD_INVOCATIONS\") {\n"
            "    let mut log = OpenOptions::new().create(true).append(true).open(path).unwrap();\n"
            "    writeln!(log, \"run\").unwrap();\n"
            "    thread::sleep(Duration::from_millis(100));\n"
            "    OpenOptions::new().append(true).open(\"rerun-trigger\").unwrap().write_all(b\"x\").unwrap();\n"
            "  }\n"
            "}\n"
        )
        (cls.package_root / "rerun-trigger").write_text("")
        (cls.package_root / "src").mkdir()
        (cls.package_root / "src/lib.rs").write_text(
            '#[test] fn noisy() { println!("application output"); }\n'
            '#[test] fn runs_from_package_directory() {\n'
            '  assert_eq!(std::env::current_dir().unwrap(), std::path::Path::new(env!("CARGO_MANIFEST_DIR")));\n'
            '}\n'
            '#[test] #[ignore] fn opt_in() { println!("ignored unless selected"); }\n'
        )
        subprocess.run(
            ["cargo", "generate-lockfile", "--offline"],
            cwd=cls.root,
            check=True,
            capture_output=True,
        )

    @classmethod
    def tearDownClass(cls) -> None:
        cls.directory.cleanup()

    def test_one_cargo_build_supplies_compile_enumerate_and_execute(self) -> None:
        build_log = self.root / "build-invocations.log"
        target_dir = self.root / "single-cargo-target"
        with (
            patch.object(run_exact_cargo_test, "ROOT", self.root),
            patch.dict(
                os.environ,
                {
                    "CARGO_TARGET_DIR": str(target_dir),
                    "GROW_EXACT_BUILD_INVOCATIONS": str(build_log),
                },
            ),
        ):
            run_exact_cargo_test.execute("grow-exact-fixture", None, "noisy")
            self.assertEqual(build_log.read_text().splitlines(), ["run"])
            subprocess.run(
                [
                    "cargo", "test", "--locked", "-p", "grow-exact-fixture", "--lib",
                    "--no-run", "--message-format=json",
                ],
                cwd=self.root,
                check=True,
                capture_output=True,
            )
            self.assertEqual(build_log.read_text().splitlines(), ["run", "run"])

    def test_test_binary_runs_from_its_package_directory(self) -> None:
        with patch.object(run_exact_cargo_test, "ROOT", self.root):
            run_exact_cargo_test.execute(
                "grow-exact-fixture", None, "runs_from_package_directory"
            )

    def test_real_noisy_test_passes(self) -> None:
        with patch.object(run_exact_cargo_test, "ROOT", self.root):
            run_exact_cargo_test.execute("grow-exact-fixture", None, "noisy")

    def test_real_missing_and_ignored_selections_are_not_green(self) -> None:
        with patch.object(run_exact_cargo_test, "ROOT", self.root):
            for name in ["renamed_missing", "opt_in"]:
                with self.subTest(name=name), self.assertRaises(run_exact_cargo_test.TestSelectionError):
                    run_exact_cargo_test.execute("grow-exact-fixture", None, name)
            run_exact_cargo_test.execute("grow-exact-fixture", None, "opt_in", ignored=True)


if __name__ == "__main__":
    unittest.main()
