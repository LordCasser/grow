## Why

`diagnostics::instrumentation::timer()` currently drops its Chrome-mode guard without creating a tracing span. Consequently, the existing timer call sites appear in log mode but do not emit timer spans in Chrome traces.

## What Changes

- Have the existing diagnostics timer create and hold a Chrome tracing span in Chrome mode.
- Give direct diagnostics timer calls a named Chrome span while preserving the shell macro's existing literal-named span path.
- Add a focused test that observes the span through the existing Chrome tracing layer.

## Scope

This restores the existing diagnostics timer's expected output mode. It adds no timing framework, changes no timer call-site API, and does not alter runtime behavior outside opt-in diagnostics. `.openspec.yaml` uses `skip_specs: true`; no product behavior contract changes.
