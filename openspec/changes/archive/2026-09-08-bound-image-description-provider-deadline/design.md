# Design
Use the earlier of the existing recovery deadline and per-call deadline at provider entry. Do not poll the provider when that deadline has expired. Preserve timeout classification, Sideband terminal handling and capability-learning rules. This change does not promise bounded filesystem I/O or durable finalization. Add focused deterministic timing coverage before selecting implementation details; reuse existing recovery integration tests.

# Implementation
The call site computes min(recovery deadline, provider entry + per-call duration). A private biased select polls the absolute deadline before the provider. Tokio Timeout polls its inner future first (confirmed in local registry source), so timeout_at alone would still poll an already-expired immediately-ready provider. The helper returns only a local timeout marker; existing Sideband fail/usage/recovery handling remains at the call site. Paused-clock tests cover expired preparation, partial preparation and successful completion.

# Test harness adjustment
The first existing image recovery test run aborted with stack overflow on its explicit 8 MiB test thread. RUST_MIN_STACK does not override Builder::stack_size. Increase this test-only stack to 32 MiB and rerun; this does not change production stack settings or establish whether the 8 MiB failure predates this change.
