# Verification

Validated with Rust 1.93.1 on macOS arm64.

- Shell: 4,266 passed, 5 explicitly ignored. Pager: 7,517 passed, 12 explicitly ignored.
- Real actor admission tests preserve Pending-to-Applied receipts while repeated Agent/Sampling selections settle without a message. Repair does not synthesize a false switch notice.
- Pager tests cover identical current/desired targets in Pending and Applying. Effort-only changes remain visible.
- The same Agent selector with changed effective profile still rebuilds and emits a real success notice.
- Formatting, diff whitespace and strict OpenSpec validation passed. See ../2026-10-19-enforce-release-regression-gates/verification.md.
