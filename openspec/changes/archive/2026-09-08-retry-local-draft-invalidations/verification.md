# Verification

## Implementation
Private invalidations map tracks each key's next retry separately from recoverable tracked records. invalidate_key removes recoverable state, preserves an existing future deadline and attempts due removal. Success/NotFound clears intent; failure rearms existing1-second backoff. next_deadline/flush_due include invalidations, and explicit flush_all attempts final cleanup. Closed-agent pruning does not discard deletion intents.

Pending invalidation skips disk restoration and empty capture. Valid new payload cancels its key's intent before normal capture/write. During binding, if either identity is invalidated, both identities are invalidated without migrating stale files; each failed old/new deletion remains independent.

## Tests
Two new real AppView tests:
- For both SendPrompt and SendPromptBlocks, persist cwd and session drafts, redirect store to a regular file as invalid root, transfer ownership and observe two pending deletions. Restore root, bind before deadline, verify both original files remain unmigrated, close/reopen under new AgentId without restoring old text, then tick deadline and verify both files removed.
- Capture oversized unsupported input with invalid root, check repeated sync preserves retry deadline; restore storage and capture valid new text, verify intent cancelled and later old deadline cannot delete the new persisted draft.

Existing local_drafts module exercises normal ownership transfer, binding-window cleanup, safe recovery, failed close/reopen, shared owner retention and source validation.

## Limits
No real GROW_HOME access, OS permission changes, Windows or installedCLI run. Failures use an invalid temporary store root; not an injected low-level remove syscall. Tests invoke actual AppView helpers and sync, not network RPC transport. In-memory intent cannot guarantee deletion after process death with sustained IO failure. Cross-process conflicting writers remain outside scope.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib local_drafts --quiet:18 passed,0 failed,0 ignored;0.21s; process exited0. Existing macOS compact-unwind linker warning only. git diff --check passed.
