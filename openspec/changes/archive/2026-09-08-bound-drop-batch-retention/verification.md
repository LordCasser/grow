## Results

`cargo test --locked --offline -p pager-render --lib prompt_images --quiet`: 160 passed, 0 failed, 0 ignored, 0.09s. Build used CARGO_INCREMENTAL=0, disabled dev/test debug info, two jobs and RUST_MIN_STACK=16777216.

New tests invoke the production classifier implementation with a small injected aggregate allowance and real temporary PNGs. They cover exact fit with an interleaved ordinary path, ordinary paths with zero allowance, one-byte overflow on space/newline/CRLF separated images, zero allowance for images, and the existing all-or-nothing prose rule. Existing per-file bounded-read, oversized-file fallback, symlink and parsing tests also pass.

The public wrapper supplies 50,000,000 bytes. Sequential admission replaces per-line collection, so no line can retain arbitrarily many candidates before checking. Candidate loading still uses the independently bounded single-file reader. The tests establish classifier behavior, not a live UI or process-RSS measurement.

Caller inspection confirmed Dashboard's existing image-only filtering and deferred sources' text_to_insert_on_miss limitation; both are recorded separately in backlog. No claim of complete clipboard fallback preservation or decoded-pixel bounds is made.
