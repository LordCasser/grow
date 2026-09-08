# Verification

## Implementation
At sync entry, remove AgentId routes absent from the authoritative agent map. Only keys with no surviving route get one closing checkpoint and loaded invalidation; successful checkpoints release tracked content, failed ones keep their due deadline. Clear retired active_key to prevent a second checkpoint from the same active-switch handling. Subsequent sync only retries by due time. End-of-sync pruning removes orphaned clean caches and loaded markers.

When loading a newly owned key with an existing pending record, restore that newer in-memory record without replacing it with disk or clearing its deadline. An active reopen using pending data avoids a redundant active-switch write before its retry deadline.

## Coverage
New real AppView tests remove/reinsert the Agent map entry under a new AgentId (the same map mutation as the production close helper; they do not dispatch the full close action):
- Saved text/cursor/Plan survives close and reopen; clean keys/loaded/tracked release; queue stays empty and disk record survives.
- Keep an old disk record, redirect store to an invalid root for closing failure, retain the latest unsaved prompt and stable retry deadline across another closed sync and active reopen, then restore storage and persist at the due time.
- Model a departed second shared-key route while a real Agent remains; only its route disappears, not shared loaded/tracked state.

Existing local draft tests cover record validation, special file/byte boundaries, AppView recovery and prompt ownership transitions.

## Limits
No installed CLI/Windows run, memory/RSS benchmark or full close-dispatch UI test. Reopen fixture reuses the removed AgentView after clearing composer/deferred state and changes AgentId to isolate draft-runtime restoration. Simultaneous editing by distinct live agents sharing one draft key is not redesigned. Disk deletion failures after RPC/capture remain separate debt.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib local_drafts --quiet:16 passed,0 failed,0 ignored;0.16s; process exited0. Existing macOS compact-unwind linker warning only. git diff --check passed.
