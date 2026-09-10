# Why

The first v2.1.6 Linux coordination job aborts in the Rust test thread `cross_workspace_approval_offers_only_one_shot_choices` with stack overflow. Unlike core-regression and the passing local shell suite, the two platform regression workflows do not set RUST_MIN_STACK. The user explicitly requested that test-thread stack limits not constrain implementation.

# What changes

Set RUST_MIN_STACK=16777216 in local-coordination and windows-session-storage job environments, matching the established core regression setting. This accommodates debug test fixtures without changing production code or removing scenarios.

# Capabilities

No product contract changes; `skip_specs: true` for CI maintenance.

# Impact

Two workflow environment values and development guidance. YAML structure and parity with the existing core setting are validated locally; actual platform pass/fail remains a gate in the release review record.
