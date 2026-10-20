# Fix the SIGTERM process-tree test fixture import

## Why

The subprocess wrapper in the exact Cargo runner's real SIGTERM cleanup test loads the runner with `importlib.util` but does not register the module in `sys.modules` before executing it. On Python 3.12, dataclass processing of postponed annotations then raises during import; the fixture exits before creating its child PID file, obscuring the import failure as a process-tree startup failure.

## What Changes

- Register the dynamically loaded runner module in `sys.modules` before executing its loader.
- When the fixture exits before writing the child PID file, include its exit code and captured stdout/stderr in the assertion failure.
- Keep the real descendant cleanup assertions unchanged.

This changes only test fixture behavior and diagnostics. No product behavior or accepted contract changes; `skip_specs: true` applies.
