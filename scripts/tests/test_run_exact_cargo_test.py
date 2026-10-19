from __future__ import annotations

import unittest
import sys
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import run_exact_cargo_test


class ExactCargoTestTests(unittest.TestCase):
    def invoke(self, ignored: bool = False) -> None:
        run_exact_cargo_test.execute(
            "pager", "pty_e2e_persistence", "module::scenario", ignored
        )

    def test_lists_unique_exact_test_then_runs_it_with_ignored_opt_in(self) -> None:
        with patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            side_effect=[
                type("Result", (), {"returncode": 0, "stdout": "module::scenario: test (ignored: PTY)\n"})(),
                type("Result", (), {"returncode": 0, "stdout": "running 1 test\ntest module::scenario ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.1s\n"})(),
            ],
        ) as run:
            self.invoke(ignored=True)
        self.assertEqual(run.call_count, 2)
        self.assertIn("--list", run.call_args_list[0].args[0])
        self.assertIn("--ignored", run.call_args_list[1].args[0])
        for invocation in run.call_args_list:
            self.assertEqual(invocation.kwargs["encoding"], "utf-8")
            self.assertEqual(invocation.kwargs["errors"], "replace")

    def test_missing_test_fails_before_execution(self) -> None:
        with patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            return_value=type("Result", (), {"returncode": 0, "stdout": "another::test: test\n"})(),
        ) as run:
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "found 0"):
                self.invoke()
        run.assert_called_once()

    def test_ambiguous_name_fails_before_execution(self) -> None:
        with patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            return_value=type("Result", (), {"returncode": 0, "stdout": "module::scenario: test\nmodule::scenario: test\n"})(),
        ) as run:
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "found 2"):
                self.invoke()
        run.assert_called_once()

    def test_cargo_listing_failure_stops_before_test_execution(self) -> None:
        with patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            return_value=type("Result", (), {"returncode": 101, "stdout": "Cargo could not build the test target\n"})(),
        ) as run:
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "could not list"):
                self.invoke()
        run.assert_called_once()

    def test_cargo_success_with_zero_executed_tests_fails(self) -> None:
        with patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            side_effect=[
                type("Result", (), {"returncode": 0, "stdout": "module::scenario: test\n"})(),
                type("Result", (), {"returncode": 0, "stdout": "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.0s\n"})(),
            ],
        ):
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "exactly one"):
                self.invoke()

    def test_successful_exit_with_ignored_test_is_not_a_pass(self) -> None:
        with patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            side_effect=[
                type("Result", (), {"returncode": 0, "stdout": "module::scenario: test (ignored: PTY)\n"})(),
                type("Result", (), {"returncode": 0, "stdout": "running 1 test\ntest module::scenario ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.0s\n"})(),
            ],
        ):
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "non-ignored test"):
                self.invoke()

    def test_nested_cargo_harness_output_still_proves_outer_test_ran(self) -> None:
        nested_output = (
            "running 1 test\ntest module::scenario ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.0s\n"
            "test module::scenario ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s\n"
        )
        with patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            side_effect=[
                type("Result", (), {"returncode": 0, "stdout": "module::scenario: test\n"})(),
                type("Result", (), {"returncode": 0, "stdout": nested_output})(),
            ],
        ):
            self.invoke()

    def test_failing_test_is_not_hidden_by_a_reported_run_count(self) -> None:
        with patch.object(
            run_exact_cargo_test.subprocess,
            "run",
            side_effect=[
                type("Result", (), {"returncode": 0, "stdout": "module::scenario: test\n"})(),
                type("Result", (), {"returncode": 101, "stdout": "running 1 test\ntest module::scenario ... FAILED\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.0s\n"})(),
            ],
        ):
            with self.assertRaisesRegex(run_exact_cargo_test.TestSelectionError, "exit 101"):
                self.invoke()


class RealLibtestSelectionTests(unittest.TestCase):
    """Exercise the parser against Rust's real test harness, including stdout."""
    @classmethod
    def setUpClass(cls) -> None:
        import tempfile
        import subprocess
        cls.directory = tempfile.TemporaryDirectory(prefix="grow-exact-test-")
        cls.root = Path(cls.directory.name)
        (cls.root / "Cargo.toml").write_text('[package]\nname = "grow-exact-fixture"\nversion = "0.0.0"\nedition = "2021"\n')
        (cls.root / "src").mkdir()
        (cls.root / "src/lib.rs").write_text('#[test] fn noisy() { println!("application output"); }\n#[test] #[ignore] fn opt_in() { println!("ignored unless selected"); }\n')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=cls.root, check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls) -> None:
        cls.directory.cleanup()

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
