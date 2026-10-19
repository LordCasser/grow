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

## First committed candidate

8a5685a45d9e7b2695b5d82c6ef4e7cdc7f1e4e6 was pushed to main. OpenSpec 39272597030 and Windows storage 39272597278 passed. Core 39272597347 reached 2424 tools tests passed, one failed, seven ignored: the original allexport test passed, but the new large-function fixture did not receive a valid first dump. No tag was created. The separate complete-shell-dump-terminators change adds a deterministic real-pipe newline regression, fixes premature marker completion and cumulative scans, and strengthens invalid-dump diagnostics. Its local final shell-state suite passes 29 tests; corrected-source hosted evidence follows rather than reusing the earlier candidate's green jobs.

## Second committed candidate

fb1beab83c8341c702ba562d927b60af374b0cdd was pushed to main. OpenSpec 39275705778 and explicitly dispatched Windows storage 39275829671 passed. Core 39275706329 stopped before tools tests on the existing config watcher late-directory/unregister test (shell: 4284 passed, one failed, seven ignored). The independently specified reject-orphaned-config-watch-events change confirms an unowned queued-event path with a deterministic red test, adds minimal ownership checks while preserving permanent/shared sources, and passes all 14 watcher tests plus 10 repeats of the failed fixture. Default CLI builds without warnings. No tag has been created and these prior failed runs do not authorize publication.


## Corrected final candidate

f497f6a10b1c25404980513732eb790c7054ccb6 passes main Core 39278978360 and native coordination 39278978850, but its first official publication run 39280878575 was cancelled before any release existed when complete Windows build logs exposed eleven first-party platform warnings. The separate [platform-warning maintenance](../2026-10-20-clear-platform-release-warnings/verification.md) preserves runtime/resource behavior, clears those warnings and makes them errors in official Windows builds.

Final source/tag commit: 5fcbd12b0c10afef1f34695394d2ad81ff5b5310. The unpublished annotated tag was replaced with an expected-object force-with-lease after cancellation and confirming release-not-found. Corrected preflight passes, and official full-platform/publication run [39290950832](https://github.com/LordCasser/grow/actions/runs/39290950832) rechecks all gates against that exact source. Both Windows release jobs pass with -D warnings and zero diagnostics, including native process smoke. No earlier-source artifacts/gates are substituted.

The platform-warning change is now archived; strict current/change validation passes 16 items, and archived-task validation passes 642 items. Temporary Cargo target cleanup removed 107009 files / 27.2 GiB without touching the user's existing workspace or global dependency caches. Vendor diagnostics and exact-runner observability are recorded separately in backlog; no vendor/MCP implementation change is included.

Corrected-source Core job 118191312675 is successful: 16467 library cases passed, zero failed, 28 existing ignored; 30 explicitly selected integration/PTY cases each reported exactly one passed non-ignored test. The default CLI build and live provider recovery/Goal/continue scenarios pass before the PTY step. All three native coordination/export jobs and Windows storage gate pass. Both macOS release logs have zero warning/error diagnostics and print grow 2.4.1 (5fcbd12); both Windows release logs also have zero diagnostics under -D warnings. Five Linux release logs and Core show no first-party Rust warnings; existing vendor/compiler/bootstrap diagnostics remain explicitly scoped as above.


## Published official release

Official run 39290950832 completed successfully with all 20 jobs successful, including publication job 118224665750. GitHub reports public, non-prerelease v2.4.1 published at 2026-10-19T18:14:26Z and selected as Latest. Remote annotated tag object 2b2a19332ee90fdf5378a5bccaaf4f6e7e5df92a peels to 5fcbd12b0c10afef1f34695394d2ad81ff5b5310. The asset set is ten platform archives plus SHA256SUMS. Publication re-downloads archives, verifies their build-one.yml attestations, checks the manifest and rechecks the tag before setting draft=false.

Coverage limits: six platform builds run native version/process coordination smoke (Linux GNU x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64/aarch64). RISC-V and both musl archives are cross-built without native process smoke. OHOS separately runs --version and sessions list inside its container before self-signing; doctor output is diagnostic, not a required health assertion. No physical OHOS device validation is claimed. Its version output is grow 2.4.1 (unknown): the build.rs Git-short-hash fallback is recorded in backlog, not presented as a verified embedded commit. External immutable source/attestation verification is separate from that display metadata.

Independent public-download verification follows below; a successful publisher does not substitute for this final local asset check.

Independent public asset checks passed: the public/non-prerelease/Latest state and remote peeled tag match; release body equals the tag's release notes; all ten downloaded archives match the remotely downloaded SHA256SUMS, contain exactly the expected regular executable and have the expected ELF/Mach-O/PE architecture. All ten independently pass gh attestation verify with both source-digest and signer-digest pinned to 5fcbd12b0c10afef1f34695394d2ad81ff5b5310 and signer-workflow LordCasser/grow/.github/workflows/build-one.yml. The downloaded macOS ARM64 executable passes codesign --verify --strict and actually returns grow 2.4.1 (5fcbd12) with an isolated temporary GROW_HOME. Structured hashes/asset sizes/proof scope are in [asset-verification.json](asset-verification.json).

Final documentation/archival follow-up changes no product source, public tag or published asset. MCP branch remains 5b375b07c08ec113c06cb9fa426bd0353e114941 and stash remains 29671011ad27541fa97a8cc649d63190532aed60. The two preserved non-MCP branches still require either unique-work disposition or removal of the app's pinned-worktree protection before deletion; no protection was bypassed.
