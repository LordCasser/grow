# Verification

## Release contents and identities

Source baseline main bdfbc11e, with the independently committed CI prerequisite/diagnostic patch 1886dd76. Workspace product version updated to 2.4.1; locked metadata matches. `cargo update --workspace --offline` changed exactly the 33 source-less local product package version fields in Cargo.lock; all external dependencies, checksums, dependency entries and the 24 independently versioned local packages are unchanged. Initial no-deps metadata did not rewrite the lock, so the explicit workspace-only update was used and structurally compared to the previous TOML lock.

Notes cover the archived v2.4.0-to-main changes, and a missing 2.4.0 index entry is restored alongside 2.4.1. No change from codex/mcp-policy-sdk-identity is merged; its HEAD remains 5b375b07c08ec113c06cb9fa426bd0353e114941 and the existing stash remains 29671011ad27541fa97a8cc649d63190532aed60. The mcp package inherits only the workspace version bump, not SDK/policy source changes. Timeline schema remains 27, as in v2.4.0.

## Branch cleanup

Six branches were proved main ancestors with no unique commits/patches, then removed locally and from origin: codex/fix-ohos-release-bootstrap, codex/release-v2.1.6, codex/release-v2.1.6-validation, merge-upstream/curated, ohos-adaptation, codex/openspec-sdd. Stale registrations for two missing worktrees were pruned only after checking their HEADs are main ancestors. The existing clean grow-openspec-sdd worktree was detached at its original HEAD; files were preserved.

MCP is retained. codex/pre-release-v2.1.10-local is retained because it has a non-equivalent unmerged commit ae1f406a. The attached codex/harden-runtime-release worktree archive was rejected by the app as protected by a pinned task/workspace, so its branch/worktree was left intact rather than bypassing that protection.

## Gate status before preparation archive

The prior bdfbc11e Core run 39264887406 failed (2406 passed, 17 failed, 7 ignored). Sixteen failures required external rg, missing from that workflow setup. The independent provide-core-regression-ripgrep change installs and checks it before tests. The remaining shell-state set-a test returned 126 without stderr in the old fixture; test diagnostics now retain it. No test is skipped or relaxed. The diagnostic Ubuntu run 39268899482 confirmed all sixteen grep failures were resolved; the remaining failure reports base64/tr Argument list too long. The independently specified Bash serializer fix isolates allexport and inherited export attributes, retaining POSIX/shopt ordering. Its final real-fd shell-state suite passed 28 tests with zero warnings/errors; controlled Bash 3.2/5.3 probes and adversarial review covered the large-function and option-order boundaries.

Preparation validation and publication results follow as they complete. No success claim is made for an unexecuted hosted gate or asset check.


## Local candidate validation

Pinned Rust 1.93.1 `cargo build --locked -p cli --bin grow` passed after the final Bash source adjustment, with zero compiler warnings/errors. The local binary reports version 2.4.1 (its build-time Git identity precedes the preparation commits and is not the final release artifact). Python regression/contract tests: 19 passed. Changed Rust rustfmt check, locked metadata and git diff whitespace checks passed. After the independent shell archive, strict current/change validation passed 17 items and archive validation passed 638 items. Preparation archive adds one archive and removes one active maintenance change; final counts are recorded below.

This local candidate is ready for committed-source CI. Main and official release gates must still pass on the immutable final source before publication; no prior-source successful job is substituted.

Final preparation validation: 16 current/change items and 639 archived changes passed.
