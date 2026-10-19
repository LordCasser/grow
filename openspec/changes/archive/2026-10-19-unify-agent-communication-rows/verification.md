# Verification

Validated with Rust 1.93.1 on macOS arm64.

- Pager: 7,517 passed, 12 explicitly ignored.
- Projection tests cover message/reply title, Received status, original body/source/reply details, unavailable body, invalid identity, child ownership and stable receipt identity across transport IDs.
- Full and cursor-tail reload tests preserve one receipt and Minimal's committed frontier.
- The complete Pager run exposed Notice-only checks in cursor merge and initial child replay eligibility. Both now recognize structured parent-message rows. Ordinary tool and outgoing communication negative cases still prevent duplicate history replay. Existing child replay assertions retain identity/body/source checks.
- The real independent-process coordination script passed ten checks: busy foreground, FIFO/deduplication, cancellation, approval/rejection, real list/ask lifecycle, restart recovery, peer identity and interrupted terminal settlement. Existing inquiry phases remain authoritative; receipt does not imply consumption or execution.
- Formatting, diff whitespace and strict OpenSpec validation passed. See ../2026-10-19-enforce-release-regression-gates/verification.md.
