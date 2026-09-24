# Verification

The existing ignored `shell --features test-support --test session_load_perf full_session_load_e2e` test was run with `--locked --offline --ignored --nocapture --test-threads=1`, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_BUILD_JOBS=2`, and the following matched synthetic knobs: `GROW_PERF_ACU_PER_TURN=8`, `GROW_PERF_CATALOG_COMMANDS=16`, `GROW_PERF_CATALOG_DESC_LEN=96`, `GROW_PERF_AGENT_CHUNKS_PER_TURN=8`, `GROW_PERF_AGENT_CHUNK_LEN=4096`, `GROW_PERF_REWIND_POINTS=2`, `GROW_PERF_FILES_PER_REWIND=2`, `GROW_PERF_FILE_CONTENT_LEN=256`. Separate invocations set `GROW_PERF_TURNS=128` and `512`. Both passed.

| Turns | Updates | Shell `session/load` round trip | First notification | Shell replay phase | Shell `load_light` | PTY resume to last visible history |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 128 | 7.7 MB | 947.6 ms | 334.1 ms | 186.4 ms | 153.2 ms | 3,041.7 ms |
| 512 | 30.7 MB | 2,284.0 ms | 1,266.5 ms | 758.2 ms | 626.2 ms | 9,053.5 ms |

The Shell test uses an in-process ACP client and the PTY test starts a separate `grow` process, so their timing difference is not an exact Pager-only phase. The comparison establishes that Shell's measured load path is shorter than the complete terminal-visible path for the same synthetic payload shape; it does not prove the remaining time is exclusively Pager layout or terminal output. On the 512-turn Shell run, 4,096 persisted catalog updates were reduced to one replayed catalog, and the load still delivered 4,611 notifications. The next measurement should time Pager receipt, layout, and terminal write boundaries under the same fixture before changing production code. Dense real tools, child sessions, resident/cursor and slow terminals remain unmeasured.

`openspec validate attribute-resume-load-phases --strict --no-interactive` and `git diff --check` passed.
