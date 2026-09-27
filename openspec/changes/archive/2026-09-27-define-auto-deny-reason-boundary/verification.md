# Verification

- Updated both stale workspace assertions to require the harness-owned Auto denial and to reject model-provided instructions in the Agent result and audit reason. Structured classifier source, verdict, denial count and escalation remain asserted.
- `cargo test --locked -p workspace --lib permission:: -- --test-threads=4`: 407 passed.
- The complete workspace library run: 871 passed, 1 unrelated environment-dependent test failed because `~/Downloads` contains `.git` while `bare_downloads_is_unsafe` assumes otherwise. No permission test failed.
