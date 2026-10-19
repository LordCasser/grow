# Verification

Candidate [8a5685a4 Core run 39272597347](https://github.com/LordCasser/grow/actions/runs/39272597347) passed the original set-a test and the POSIX/shopt test, but failed the new large-function test on the first snapshot-update assertion. Tools: 2424 passed, one failed, seven ignored. The earlier code/status/function/export/flag assertions passed. Since the old helper discarded invalid dump details, that run alone does not distinguish timeout from a marker boundary failure.

An independent debug Rust probe measured approximately 0.39 seconds for the old cumulative marker scan on 1.6 MB locally, and 14.6 seconds on 10 MB. This confirms quadratic growth, not a proof that the smaller Ubuntu fixture timed out.

## Real-pipe red/green

Pinned Rust 1.93.1, target `/tmp/grow-241-dump-boundary-target`. The new `dump_reader_waits_for_terminator_newline_without_waiting_for_eof` failed on the original production reader: zero passed, one failed. It writes the END marker without the newline while retaining the writer, so premature completion is deterministic. After the suffix fix, all **29 shell_state tests passed** with no ignored tests or compiler warnings/errors. The new pipe test covers Bash/Zsh and verifies completion after the newline while the write handle remains open. Existing large-function two-round tests remain unchanged in size and enabled; allexport, exported-name collisions, POSIX/shopt, command failures and other shell-state cases pass.

The helper now asserts valid state updates and reports only byte length, start/end completeness, elapsed time, exit code and stderr when invalid; it does not print encoded environment payloads. The reader retains its original five-second deadline and incomplete-state fallback. Detection checks a fixed complete line suffix rather than the growing whole payload.

Read-only adversarial review confirmed cross-read markers/newlines, complete single reads, no-EOF completion and parser alignment. Extra data written after the marker is not a valid terminal dump under the existing parser and is not newly accepted. No claim is made that arbitrary background writes to the dedicated state descriptor are supported. The precise cause of the earlier hosted invalid dump remains unobserved; final same-source hosted validation is still required.

Default CLI build (`cargo build --locked -p cli --bin grow`) passed after the final source change, with zero compiler warnings/errors. Python tests: 19 passed. Rustfmt and git diff checks passed. Final strict current/change validation: 16 passed; archived validation: 640 passed.
