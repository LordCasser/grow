## Verification

- Added `chrome_timer_emits_a_timed_span`, which builds the existing `tracing_chrome` layer with a temporary output file and checks begin/end events. It constructs a Chrome-mode timer through the shared diagnostics timer constructor; the test does not replace or exercise the shell macro's literal-name span path.
- Root-coordinated diagnostics test run, `/tmp/diagnostics-final2.log`: 54 passed.
- `rustfmt --edition 2024 --check crates/codegen/diagnostics/src/instrumentation.rs crates/codegen/shell/src/instrumentation.rs`: passed.
- `openspec validate emit-chrome-instrumentation-timers --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed in the documentation closeout run (33 items).
- `git diff --check` for this change's source and OpenSpec files: passed.
