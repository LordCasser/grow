# Design

The test embeds a small Python wrapper that dynamically imports `run_exact_cargo_test.py` so it can substitute only `execute` and launch a real descendant process tree. Insert the module into `sys.modules` under `spec.name` before `exec_module`, matching normal import semantics expected by `dataclasses` when annotations are postponed.

If the child PID marker is absent at the startup deadline, stop the wrapper, collect its output, and report the return code and diagnostics. The signal delivery and descendant-survival assertions remain mandatory and unchanged.

## Validation

Run the focused SIGTERM test and the complete `scripts/tests` discovery under Python 3.12 and the configured default Python, with bytecode generation disabled.
