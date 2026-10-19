from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
RELEASE = (ROOT / ".github/workflows/release.yml").read_text()
WINDOWS = (ROOT / ".github/workflows/windows-session-storage.yml").read_text()
CORE = (ROOT / ".github/workflows/core-regression.yml").read_text()

GATES = {
    "core-regressions": ".github/workflows/core-regression.yml",
    "native-coordination-regressions": ".github/workflows/local-coordination.yml",
    "windows-storage-regressions": ".github/workflows/windows-session-storage.yml",
    "openspec-validation": ".github/workflows/openspec.yml",
}
PUBLISH_DEPENDENCIES = {
    "validate",
    "select",
    "build",
    "dispatch-guard",
    *GATES,
}


def job_block(source: str, name: str) -> str:
    match = re.search(
        rf"^  {re.escape(name)}:\n(?P<body>(?:(?:    .*|)\n)*)",
        source,
        re.MULTILINE,
    )
    if not match:
        raise AssertionError(f"workflow job {name!r} is missing")
    return match.group("body")


def trigger_paths(source: str, trigger: str) -> list[str]:
    match = re.search(
        rf"^  {re.escape(trigger)}:\n"
        rf"(?P<header>(?:    .*\n)*?)    paths:\n"
        rf"(?P<body>(?:      - .*\n)+)",
        source,
        re.MULTILINE,
    )
    if not match:
        raise AssertionError(f"{trigger} path filter is missing")
    return re.findall(r'^      - "?([^"\n]+)"?$', match.group("body"), re.MULTILINE)


def release_contract_errors(source: str) -> list[str]:
    errors: list[str] = []
    try:
        publish = job_block(source, "publish")
    except AssertionError as error:
        return [str(error)]

    needs_match = re.search(r"^    needs:\n(?P<body>(?:      - .+\n)+)", publish, re.MULTILINE)
    needs = set(re.findall(r"^      - ([a-z0-9-]+)$", needs_match.group("body"), re.MULTILINE)) if needs_match else set()
    missing = PUBLISH_DEPENDENCIES - needs
    if missing:
        errors.append(f"publish does not depend on: {', '.join(sorted(missing))}")

    condition = re.search(r"^    if:[^\n]*\n(?P<body>.*?)(?=^    runs-on:)", publish, re.MULTILINE | re.DOTALL)
    expression = condition.group("body") if condition else ""
    if "!cancelled()" not in expression or "always()" in expression or "inputs.publish" not in expression:
        errors.append("publish condition must reject workflow cancellation and require publish=true")
    for dependency in PUBLISH_DEPENDENCIES:
        if f"needs.{dependency}.result == 'success'" not in expression:
            errors.append(f"publish condition does not require {dependency} to succeed")

    for gate, workflow in GATES.items():
        try:
            block = job_block(source, gate)
        except AssertionError as error:
            errors.append(str(error))
            continue
        if f"uses: ./{workflow}" not in block:
            errors.append(f"{gate} does not invoke {workflow}")
        if "commit: ${{ needs.validate.outputs.commit }}" not in block:
            errors.append(f"{gate} is not bound to validate.outputs.commit")
    return errors


class ReleaseRegressionContractTests(unittest.TestCase):
    def test_core_installs_and_checks_ripgrep_before_library_tests(self) -> None:
        install = CORE.index("- name: Install ripgrep for search tool tests")
        tests = CORE.index("cargo test --locked --lib")
        install_step = CORE[install:tests]
        self.assertIn("sudo apt-get install --yes ripgrep", install_step)
        self.assertIn("command -v rg", install_step)
        self.assertIn("rg --version", install_step)

    def test_windows_filter_covers_direct_build_inputs_for_both_source_triggers(self) -> None:
        pull_request_paths = trigger_paths(WINDOWS, "pull_request")
        push_paths = trigger_paths(WINDOWS, "push")
        self.assertEqual(pull_request_paths, push_paths)
        for path in (".cargo/**", "Cargo.toml", "Cargo.lock"):
            with self.subTest(path=path):
                self.assertIn(path, pull_request_paths)

    def test_windows_release_gate_remains_path_independent_and_mandatory(self) -> None:
        self.assertRegex(WINDOWS, r"(?m)^  workflow_call:\n    inputs:")
        self.assertEqual(release_contract_errors(RELEASE), [])

    def test_publish_dag_is_fail_closed_and_commit_bound(self) -> None:
        self.assertEqual(release_contract_errors(RELEASE), [])

    def test_missing_gate_dependency_is_rejected(self) -> None:
        changed = RELEASE.replace("      - windows-storage-regressions\n", "", 1)
        errors = release_contract_errors(changed)
        self.assertIn(
            "publish does not depend on: windows-storage-regressions",
            errors,
        )

    def test_a_skipped_gate_cannot_be_treated_as_success(self) -> None:
        changed = RELEASE.replace(
            "needs.windows-storage-regressions.result == 'success' &&",
            "",
            1,
        )
        errors = release_contract_errors(changed)
        self.assertIn(
            "publish condition does not require windows-storage-regressions to succeed",
            errors,
        )

    def test_workflow_cancellation_cannot_be_bypassed(self) -> None:
        changed = RELEASE.replace("!cancelled()", "always()", 1)
        errors = release_contract_errors(changed)
        self.assertIn(
            "publish condition must reject workflow cancellation and require publish=true",
            errors,
        )

    def test_reusable_gate_must_receive_the_validated_commit(self) -> None:
        changed = RELEASE.replace(
            "commit: ${{ needs.validate.outputs.commit }}",
            "commit: ${{ github.sha }}",
            1,
        )
        errors = release_contract_errors(changed)
        self.assertIn(
            "core-regressions is not bound to validate.outputs.commit",
            errors,
        )

    def test_each_reusable_gate_checks_out_and_verifies_its_commit(self) -> None:
        for workflow in GATES.values():
            with self.subTest(workflow=workflow):
                text = (ROOT / workflow).read_text()
                self.assertIn("required: true\n        type: string", text)
                self.assertIn("ref: ${{ inputs.commit || github.sha }}", text)
                self.assertIn("EXPECTED_COMMIT: ${{ inputs.commit || github.sha }}", text)
                self.assertIn('[[ "$actual" == "$EXPECTED_COMMIT" ]]', text)


if __name__ == "__main__":
    unittest.main()
