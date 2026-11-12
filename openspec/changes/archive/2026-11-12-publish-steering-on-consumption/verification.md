# Verification

## Incident evidence

Read the original session journal without changing it. Sequences 21667–21702 show a FIFO follow-up rerouted to the active Turn and a direct steer admitted to the same Turn. The foreground tool and Turn were cancelled, both inputs returned to FIFO, and each original input was consumed once. The second input was steered into the successor Turn. The journal does not contain two consumed copies of either input.

## Regression

macOS arm64, Rust 1.93.1, locked and offline builds.

- `cargo test --offline --locked -p pager -p shell --lib interjection` passed 16 Pager tests and 17 Shell tests. Verified durable display identity, live and replay reconciliation in both directions, queue promotion, direct steering, residual reroute and checkpoint restore. The Pager test `steer_targets_the_existing_turn_and_emits_no_new_prompt` also passed.
- The `pty_e2e_queue` case `cancelling_blocked_foreground_displays_each_pending_steer_once` runs a real foreground terminal tool, queues one shared text prompt and one local image prompt, uses empty Enter to steer each, and cancels with Ctrl+C. The original binary showed transcript counts `[2, 2]` while the final mock model request contained each message once. A build with only the display change showed `[1, 1]` but failed because a shared queue prompt returned to the composer. The final regression asserts `[1, 1]`, no restored queued draft, one of each input in the model request, and the original image still present.
- The final `cargo build --offline --locked -p cli --bin grow` binary passed the cancellation PTY regression and `interjection_reaches_model_in_same_turn`. The cancellation case displayed each message once, left the composer clear, and preserved the attachment and one copy of each user input in the final request. The same-turn case confirmed delivery without a new logical Turn.
- `cargo test --offline --locked -p pager --lib cancel` passed 146 tests on the final source, including pristine rewind, combined prompt rewind, newer drafts, and ordinary queue cancellation. The Pager interjection filter and existing-turn dispatch case also passed again on the final source.
