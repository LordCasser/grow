# Verification

Rust 1.93.1. Built the final test executables with `cargo test --offline --locked -p chat-state -p tools -p shell --lib --no-run`.

- ChatState library suite had 652 passing tests and 3 ignored. The runtime tree projection covers admitted Seed, Spawn and Ended events, unrelated payloads and cold replay. The legacy CommandsSettled format still deserializes.
- Tools selection had 58 passing tests across ChannelBackend, public wait/result and completion routing. A paused runtime query and inspect both return a verified history terminal without consuming the shared deadline. The selection checks that history errors are preserved on fallback, that the call still times out when no observation returns and that repeated answer reads remain usable.
- Shell selection had 11 passing tests across canonical observation, durable receipt admission, answer delivery and suspension recovery. The production tool result bridge suppresses both a pre-existing and a late receipt in the live fold and in cold replay. The other child remains pending. The command projector reproduces a 70-second command with a 60-second foreground interval and preserves the fallback for an older settlement. Suspension retry and receipt reuse also pass.
- `git diff --check` passed. Shell reported an existing irrefutable-pattern warning in input_admission.rs.

Long-context verification used the indexed projection test with synthetic unrelated history. Command timing was verified at the history projector, with the terminal snapshot source checked in code. No separate large-session benchmark or PTY run was performed.
