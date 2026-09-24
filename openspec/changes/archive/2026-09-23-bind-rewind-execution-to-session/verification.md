# Verification

- `cargo test --locked --offline -p pager --lib app::root::dispatch::tests::rewind`: 37 passed. This includes late success after replacement, late rejection/unknown outcome after same-ID rebind, Minimal notice visibility, and existing view-switch/inline-resubmit behavior.
- `cargo check --locked --offline -p pager`: passed.
- `cargo fmt --all`: passed.
- `git diff --check`: passed.
- `openspec validate bind-rewind-execution-to-session --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: 16 passed before archive.

The execution result is fenced by source session ID and binding epoch. On a binding change, the old rewind interaction closes and unsent composer/inline-edit text remains in the local composer. A stale execution result never truncates or submits through the replacement binding; confirmed success, explicit rejection and unknown transport outcome receive distinct feedback.
