# Verification

The focused `replay_automatic_draws_use_slow_cadence_until_load_completes` unit test passed. It checks the 100 ms floor, a slower configured interval, the switch back to the normal interval, and shortening an already deferred replay draw. The existing `loading_replay_preserves_typed_prompt_until_session_loaded` Pager unit regression passed after the final paint change. `rustfmt` on the changed event-loop source, `openspec validate throttle-resume-replay-paint --strict --no-interactive`, and scoped `git diff --check` passed.

The current `grow` binary was rebuilt with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo build --locked --offline -p cli --bin grow`. The same ignored 128/512-turn PTY fixture ran against that binary. The second run invoked its already compiled test executable directly after a concurrent clipboard source edit made a fresh Cargo test compile fail; the fixture executable only constructs test sessions and drives the current `grow` child. The clipboard source error was repaired and its 57 focused macOS tests subsequently passed (three real-pasteboard tests stayed ignored). No new PTY harness code was needed.

| Turns | Baseline history visible | Final history visible | Baseline render calls / sum | Final render calls / sum | Final Shell load / replay | Final key echo p95 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 128 | 3,046 ms | 2,352 ms | 112 / 1,217 ms | 47 / 371 ms | 1,550 / 1,119 ms | 7.7 ms |
| 512 | 9,354 ms | 6,108 ms | 253 / 4,360 ms | 71 / 1,021 ms | 5,754 / 4,459 ms | 7.8 ms |

The intermediate ACP/animation-only build still painted 81/203 frames and reached history in 2,879/8,940 ms. Periodic UI-maintenance paint requests were the remaining high-frequency trigger. The final build retains maintenance work but schedules its paint through the same replay-only cadence. Timer sums overlap Shell work and include post-load key-echo frames; they are not additive wall-clock phases. This synthetic fixture does not cover dense tool histories, multiple child sessions, resident/cursor memory, or slow terminals.
