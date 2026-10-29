## Design

Use the existing `InstrumentationTimer` and `tracing_chrome` layer. In Chrome mode, the public `timer()` constructor creates an entered span named `instrumentation.timer`, targeted at `grow_instrumentation`, and records the runtime timer name as a span field; dropping the timer closes the span and lets the Chrome layer record its duration. The shell macro retains its existing literal-named span path. Log and disabled modes keep their current behavior.

The timer function signature and existing `InstrumentationTimer::new_with_span` API remain unchanged; direct timers and macro timers each create exactly one Chrome span.

This changes opt-in diagnostics output only, not an archived product contract. `.openspec.yaml` sets `skip_specs: true`.
