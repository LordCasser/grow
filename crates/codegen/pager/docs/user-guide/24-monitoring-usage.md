# Local Diagnostics

Grow records diagnostics only through the local `tracing` pipeline. It does not
ship telemetry, product analytics, Sentry events, OTLP data, session metrics, or
trace archives to a remote service.

## Enable local logs

- `grow --debug` enables the debug firehose at `$GROW_HOME/debug/firehose.txt`
  (by default, `~/.grow/debug/firehose.txt`). All processes append to this file;
  each line includes `sid=<session-id>` when it belongs to a session.
- `GROW_DEBUG_LOG=1` enables the same firehose for the TUI and agent processes.
- `--debug-file <path>` writes to that exact file and bypasses the default
  firehose.
- `GROW_LOG_FILE` writes logs to the exact path provided.
- `RUST_LOG` controls the `tracing` filter, for example
  `RUST_LOG=shell=debug,diagnostics=info`.

The local log contains tracing events with session attribution, including
structured events under the `diagnostics` target. Useful events cover model requests, tool execution,
permissions, MCP lifecycle, compaction, goals, subagents, and authentication
errors. Each log file retains at most 32 MiB of recent complete lines; a single
line is limited to 64 KiB, and queue overflow appears as a dropped-record count.
Event payloads remain on the local machine.

Grow intentionally has no `[telemetry]`, webhook, OTLP, or upload configuration.
The optional `[diagnostics] crash_handler = true` setting only enables local crash
capture; it does not add a network sink. Forwarding a local log elsewhere is an
operator-owned action outside Grow.
