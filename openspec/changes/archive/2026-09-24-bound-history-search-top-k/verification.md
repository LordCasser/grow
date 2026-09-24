`cargo test --locked -p pager --lib history_search -- --test-threads=1` passed: 21 tests, 0 failures (7,313 filtered out). The linker emitted an existing toolchain warning that `.eh_frame` exceeded the compact-unwind encoding limit; the test binary linked and ran successfully.

`openspec validate bound-history-search-top-k --strict --no-interactive` passed. `git diff --check` passed for the implementation, backlog, and change files.
