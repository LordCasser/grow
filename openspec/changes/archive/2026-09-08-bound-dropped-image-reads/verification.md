## Verification

`cargo test --locked --offline -p pager-render --lib prompt_images --quiet`: 158 passed, 0 failed, 0 ignored (0.10s), using disabled incremental/debug information and two build jobs.

New tests verify that the reader consumes at most limit + 1 bytes, preserves an exact-limit payload, and rejects empty or oversized input. A sparse 50,000,001-byte image path remains a NonImage path reference. Existing image_source_path_preserves_user_visible_symlink passes alongside valid image and path parsing tests.

Production callers were confirmed in Pager agent_view/paste.rs and dashboard/state.rs. This change bounds the actual drag/path-paste reader, separate from the unused legacy send builder under deletion review.

The limit concerns encoded bytes. It does not establish an I/O deadline, a decoded-pixel budget, or immutable file contents. No live clipboard/UI operation or Windows cross-test was performed.
