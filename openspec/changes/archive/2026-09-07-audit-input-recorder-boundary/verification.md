# Source audit

- InputRingBuffer::new fixes capacity to 200; fields are private and repository search finds no alternate struct construction or mutation outside the module. push drops the oldest at capacity; existing capacity test covers 250 pushes. There is no user-provided zero-capacity configuration to fix.
- push uses Instant::now; snapshots and time_span_ms compare insertion-ordered monotonic instants. Wall timestamps are separately captured with SystemTime and unwrap_or_default before the epoch. No wall-clock subtraction drives relative durations.
- record_input retains raw KeyCode in the bounded in-memory ring, while snapshot_entries uses sanitize_key_code: printable chars become Char, with BS/DEL control categories retained. This protects output character values, not arbitrary in-process memory or metadata such as session ID and terminal information.
- Actual reachable dump action is AgentView input.rs: Press Esc arms a timer, followed by unmodified d Press under 500ms emits Action::DumpInputLog, subject to earlier input handlers. The prior Ctrl+Shift+D module comment did not match the sole production Action producer and is corrected.
- format_key_code_raw is cfg(test) and its only calls across crates are in format_key_code_raw_shows_punctuation. It verifies a raw representation unused by production serialization or other tests. Added R13 to the user-confirmation list; retained implementation and test.

## Validation
Read-only source searches and comment review. No new Rust behavior tests or CLI build were needed/run; the recorder's 8 tests passed in the immediately preceding behavior fix. git diff --check and OpenSpec validation performed for this change. No deletion is authorized or performed.
