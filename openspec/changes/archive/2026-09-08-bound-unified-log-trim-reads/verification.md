# Verification
- main; low-disk `cargo test --locked --offline -p diagnostics --lib unified_log::tests`: 20 passed, 0 failed/ignored, 0.01s. CARGO_INCREMENTAL=0, dev/test debug=0, jobs=2, RUST_MIN_STACK=16777216.
- Counted Read+Seek proves actual 2.5 MiB byte consumption and excludes bytes beyond captured length. Sparse file above 15 MiB trims to complete recent lines while preserving identity. Existing small half/no-newline/missing-file/inode/open-writer/concurrent-trimmer/healing tests pass.
- New tests use explicit temporary paths; existing suite installs pre-main unified-log temp redirection. No real GROW_HOME write or installed binary replacement.
- Limits: no newline still means no trim, appends not locked against rewrite remain existing behavior, no hard disk cap or bounded snapshot/serialization guarantee. Read payload is capped, not an exact total allocator/RSS cap.
