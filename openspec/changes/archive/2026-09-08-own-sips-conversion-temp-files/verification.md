## Results

`cargo test --locked --offline -p pager-render --lib terminal::image::tests --quiet`: 14 passed, 0 failed, 0 ignored (0.07s), using disabled incremental/debug info and two jobs.

New tests exercise the same owned conversion helper with local commands: successful output copy, output creation followed by exit7, successful exit without output, output directory instead of a regular file, missing executable, and blocked source-file creation. Every case checks the owned directory is gone after return. Separate tests check distinct workspace paths, Unix mode0700, and that releasing one workspace leaves the other intact. Existing JPEG-to-PNG conversion and pixel-budget tests also pass.

No global temporary directory or environment override was used. Input write/sync faults and an actual output read syscall fault were not injected; those use the same owned scope and RAII error exits. Process death, filesystem cleanup refusal and escaped descendant behavior are not covered by this cleanup claim. No Windows execution was performed. Timeout and output-byte limits remain separate work.
