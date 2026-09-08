## Verification
`cargo test --locked --offline -p pager --lib live_image_loader --quiet`: 2 passed (0.00s).
`cargo test --locked --offline -p pager --lib image_loader --quiet`: 4 passed (0.01s), including both new tests and existing count/missing/per-file/aggregate rejection plus no-placeholder-guessing cases. Filters overlap. Existing macOS compact-unwind warning, test exit0.

- Same opaque encoded bytes and URI produce equal ACP serialization from memory and disk, preserving the seeded text block.
- Removing the saved file leaves the memory-backed image usable and rejects the disk-only source.
- Unix symlink, directory and FIFO are rejected. A two-second test-side channel bound protects the regression test from a future blocking FIFO open; each current worker completed and was joined.

Only isolated temporary files were used. The test payload is opaque transport data; server image format/pixel validation is deliberately not claimed by this test. No Windows runtime, same-size concurrent write or kernel-level filesystem stall was tested. The implementation was unchanged, so no behavioral delta or new production policy is claimed.
