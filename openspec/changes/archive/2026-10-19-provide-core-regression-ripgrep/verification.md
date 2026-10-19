# Verification

- Prior core run `39264887406` finished with 2406 passed, 17 failed, and 7 ignored. Sixteen failures were in Grow grep runtime tests; the remaining failure was the separate Bash `USER_CMD` test.
- The source-build resolver uses `RG_BIN_PATH` or `rg` on `PATH`. If `rg_available()` cannot resolve it, the grep runner returns `exit_code = -1` and empty stdout, matching the reported grep failure signature. `core-regression.yml` had no ripgrep installation or executable check before the library tests. The workflow now installs the Ubuntu package and checks both PATH resolution and `rg --version` before those tests.
- `python3 -m unittest discover -s scripts/tests -p test_release_regression_contract.py`: 9 passed, including the new install/check ordering contract.
- `openspec validate --all --strict --no-interactive`: 18 passed, 0 failed.
- No Cargo suite or hosted workflow was rerun. The existing grep tests remain enabled; this record does not claim hosted confirmation after the workflow fix.

## Bash `set -a` failure diagnostic

- In the same CI log, `computer::local::shell_state::tests::test_user_cmd_var_not_exported_under_allexport_bash` fails at its first `run_command("set -a")` status assertion with exit code 126. The later `printenv __grow_user_cmd` assertion does not run.
- The test helper previously discarded child stderr. It now returns stderr, and the failing assertion includes command stage, stdout and stderr. This is diagnostic instrumentation only; the exit-126 cause remains undetermined and is not claimed as fixed.
- `rustfmt --check --edition 2024 crates/codegen/tools/src/computer/local/shell_state.rs`: passed.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -p test_release_regression_contract.py`: 9 passed.
- `openspec validate --all --strict --no-interactive`: 18 passed, 0 failed; `git diff --check` passed.
- Cargo and hosted CI were not run. The cause of exit 126 remains unknown pending the next Linux gate's stderr.
