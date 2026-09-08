# Verification
- main; explicit temporary directories and exact child process only. No real GROW_HOME/debug mutation.
- Before fix, pruning_spares_old_open_writer_across_processes failed in child with `open idle log was unlinked`.
- After fix, low-disk `cargo test --locked --offline -p diagnostics --lib debug_log::tests`: 24 passed, 0 failed/ignored, 0.01s. Child real prune preserves parent worker file older than seven days; dropping worker guard then pruning removes it. Concurrent shared writer and existing routing/link/retention tests also pass.
- CARGO_INCREMENTAL=0, dev/test debug=0, jobs=2, RUST_MIN_STACK=16777216. macOS only; Windows and unsupported/network filesystems not validated. No protection for noncooperating writers/path replacement attacks, no periodic or byte-quota claim. Read+append access now required by shared-lock writer setup.
