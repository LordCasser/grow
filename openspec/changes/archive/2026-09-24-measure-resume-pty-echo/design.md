# Design

Reuse `shell::session::testkit::synth::prepare_session` to create a session under `ContentController`'s isolated home, then open `grow --continue` in `PtyHarness`. Record from process spawn to a unique last-turn marker visible on screen. Type one character at a time in the resumed prompt, waiting for each growing prefix to appear, and report the sorted p95 across 20 samples. This measures user-observable terminal echo, including the harness pump, not just Shell load.

Keep the fixture explicit and bounded. Use the existing mock provider configuration and avoid external inference. This harness does not itself prove real-session behavior, resident cursor replay, or the other transcript shapes in backlog; those require follow-up measurements before the item can close. Do not make performance timing a default CI assertion on variable hosts.
