# Local Diagnostics

Grow records diagnostics only through the local `tracing` pipeline. It does not
ship telemetry, product analytics, Sentry events, OTLP data, session metrics, or
trace archives to a remote service.

## Enable local logs

- `grow --debug` enables the debug log for the current process.
- `GROW_DEBUG_LOG` selects the debug-log path used by the TUI.
- `GROW_LOG_FILE` writes logs to the exact path provided.
- `RUST_LOG` controls the `tracing` filter, for example
  `RUST_LOG=grow_shell=debug,grow_diagnostics=info`.

The local log contains ordinary spans plus structured events under the
`grow_diagnostics` target. Useful events cover model requests, tool execution,
permissions, MCP lifecycle, compaction, goals, subagents, and authentication
errors. Event payloads remain on the local machine and follow the same file
permissions and retention policy as the selected log file.

Grow intentionally has no `[telemetry]`, `[diagnostics]`, webhook, OTLP, or
upload configuration. Forwarding a local log elsewhere is an operator-owned
action outside Grow.
