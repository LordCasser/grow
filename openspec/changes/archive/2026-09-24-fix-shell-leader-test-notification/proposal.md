# Fix leader stdio integration test frame selection

## Why

The sever-and-replay integration test assumes the next agent-channel frame is the `session/new` RPC. The server may first emit its id-less `grow/internal/queue_client_disconnected` notification, so unwrapping the next frame's JSON-RPC id panics before the behavior under test begins.

## What Changes

Make this test wait for the expected forwarded RPC method while tolerating the unrelated disconnect notification. This changes only test fixture frame selection and does not change the ACP protocol or product behavior.

## Contract impact

None. This is test maintenance only; `skip_specs: true` avoids inventing a behavior delta.

## Verification

- `cargo test --locked --offline -p shell --test test_leader_stdio_integration test_sever_mid_rpc_orphans_response_and_replay_recovers -- --nocapture` — passed (1 test).
- `openspec validate --all --strict --no-interactive` — passed.
