## Results

`cargo test --locked --offline -p pager-render --lib terminal::image::tests --quiet`: 16 passed, 0 failed, 0 ignored (1.01s), with incremental/debug info disabled and two jobs.

New Unix tests verify normal exit0/exit7 statuses. A stalled shell with a delayed-writing descendant receives a50ms test deadline and returns TimedOut within2seconds; a normally exited leader also has its delayed-writing descendant stopped. Neither writes its marker after450ms. These exercise the production runner, not a mocked process status. Existing workspace cleanup, early failure, pixel budget and real tiny JPEG conversion tests also pass.

Production supplies10seconds to the same runner. Static review confirms it detaches the child, attaches the existing ProcessGroup, handles attachment failure by direct kill/wait, kills the group after try_wait termination, and kills/waits a leader after error. Unix ESRCH is the sole ignored cleanup error; other failures propagate to logged conversion failure. No cleanup-permission failure was injected and no Windows execution was performed.

The deadline is not an absolute wall-clock bound across OS spawn, file I/O or uninterruptible kernel waits. Escaped groups/denied termination are not guaranteed reclaimed. The sips output file read budget remains separately backlogged.
