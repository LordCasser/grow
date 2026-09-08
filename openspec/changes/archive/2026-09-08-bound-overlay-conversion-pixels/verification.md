## Results

`cargo test --locked --offline -p pager-render --lib terminal::image::tests --quiet`: 11 passed (0.04s). New tests modify a tiny JPEG's SOF dimensions to 4000x4000, 4001x4000 and 65535x65535; the existing unrestricted header validator confirms those actual dimensions. The admission predicate accepts exactly 16 million pixels and rejects larger headers. Rejected conversion returns None; direct PNG bytes remain unchanged. Empty/invalid bytes are rejected. Existing tiny JPEG-to-PNG conversion passes.

`cargo test --locked --offline -p pager-render --lib prompt_images --quiet`: 160 passed (0.08s), including preview readiness/failure and retained send-byte coverage. Only a helper doc-comment correction followed the terminal test run.

All builds use disabled incremental/debug information, two jobs and RUST_MIN_STACK=16777216. No large pixel buffer was generated. Header fixtures are not full valid large JPEG rasters and are not a conversion memory benchmark. Source ordering verifies the predicate precedes both sips and Rust decoding. No Windows, live terminal rendering or process-RSS test was run.

This is a source-pixel admission bound for non-PNG conversion. Direct encoded transmission, conversion output bytes, sips execution duration and temporary ownership remain distinct boundaries; newly confirmed sips debts are recorded in backlog.
