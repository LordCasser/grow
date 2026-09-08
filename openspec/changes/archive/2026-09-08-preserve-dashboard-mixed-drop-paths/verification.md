## Results

`cargo test --locked --offline -p pager --lib views::dashboard::state::tests --quiet` initially returned 242 passed and 1 failed. The old missing-file-URL test expected FullMiss/empty text. The shared classifier intentionally recognizes explicit missing URLs as NonImage paths, so the new insertion returns Handled with path text. The delta now states that behavior and the retained test verifies text plus absence of images.

Rerun: 243 passed, 0 failed, 0 ignored (0.38s). Includes new six-case dispatch/peek × bracketed/key/deferred ordering test, both-target image-cap test, and existing question-mode, closed-panel, moved-row and deferred-discard tests.

The missing-file-URL fixture was subsequently made portable by generating a URL for a missing file in a temporary directory instead of using a fixed Unix path. `cargo test --locked --offline -p pager --lib completion_preserves_unreadable_file_url_as_path_text --quiet`: 1 passed (0.03s). No production changes followed the broad green run.

All builds used disabled incremental/debug info, two jobs and RUST_MIN_STACK=16777216. The linker emitted the existing macOS compact-unwind size warning; final runs exited 0. No live clipboard or Windows execution was performed.

Source review confirms all three insertion sites use insert_dropped_paths. The remaining image-only classification is the question-mode discard-feedback predicate, not an insertion path. Unclassified asynchronous URLs without original text remain separately backlogged.
