# Verification

- Parsed core-regression, local-coordination and windows-session-storage with Ruby YAML and asserted every job's RUST_MIN_STACK is exactly `16777216`: passed.
- `git diff --check`: passed.
- The application's release stack and every regression scenario remain unchanged. This configuration validation does not claim remote execution success; rerun results are tracked in the v2.1.6 publication verification.
