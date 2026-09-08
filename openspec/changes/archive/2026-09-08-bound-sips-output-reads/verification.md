## Output-read verification

`cargo test --locked --offline -p pager-render --lib terminal::image::tests --quiet`: final20passed,0failed,0ignored (1.01s), with incremental/debug info disabled and two jobs.

New coverage verifies exact8-byte data, empty/oversized input and injected I/O failure; a counted Cursor stops at limit+1. A real file is metadata-checked at4bytes, appended by another handle, and bounded reading stops at5bytes. A sparse100,000,001-byte conversion output is rejected without allocating that payload and its owned directory is removed. Existing command tests now include empty output and symlink output rejection. FIFO loading returns None within a test-side2second guard. Normal tiny JPEG conversion, workspace cleanup and runner cases pass.

## Intermittent process failure retained for investigation

The first19-test run passed. After adding the real-file growth case, one20-test run produced19passed/1failed: the pre-existing normal-exit runner test returned raw EPERM. The output-growth case passed. Temporary stage diagnostics and200 alternating normal exits then passed independently and within the full module, without reproducing or attributing the error. Instrumentation/repetition was removed before the final20-test green run.

This does not prove the process failure resolved. It is separately active as investigate-sips-process-eperm; no suppression or production runner behavior change was included here.

This bound concerns Rust reads of completed sips output. It does not limit disk output before checking, all allocations/RSS or the Rust fallback encoder. No Windows execution or kernel I/O deadline was tested.
