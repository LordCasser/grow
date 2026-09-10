# Why

Release validation reproduces a native macOS unoptimized CLI stack overflow before an ordinary prompt reaches the loopback model. The actual session thread explicitly requests 8 MiB, so increasing the test harness RUST_MIN_STACK cannot affect it. The user clarified that test stack constraints should not drive implementation changes.

# What changes

Give debug-assertion builds 32 MiB of session-thread stack to accommodate unoptimized async temporary frames. Preserve the release build setting of 8 MiB and the existing conversation-turn call chain. Do not refactor production logic to satisfy debug-code stack layout.

# Capabilities

No behavior contract changes. This only adjusts a development-build resource setting; `skip_specs: true`. The same coordination behavior is validated without changing or deleting its scenarios.

# Impact

The existing stack constant in `shell/src/session/actor/spawn.rs`, development guidance and this record. No runtime protocol, persistence or release-stack change.
