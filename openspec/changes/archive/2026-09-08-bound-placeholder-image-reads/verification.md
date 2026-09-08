## Verification
`cargo test --locked --offline -p client-support --lib placeholder --quiet`: 49 passed, 0 failed (0.01s).

- Counted Cursor verifies source lengths0/8/9/128 under budget8 consume min(length,9), preserve accepted bytes and report observed9 on excess. Injected PermissionDenied remains ReadFailed.
- Real PNG at exact cap loads successfully. A file handle inspected before an append is then read through the bounded helper; it returns TooLarge and advances only original cap+1 bytes.
- Existing static metadata oversize test still reports the complete metadata size. Existing canonicalization, prefix/deny rules, extension, MIME, recovery/dedup and aggregate-budget tests pass.
- Production load_canonical_placeholder_image keeps the existing prechecks, opens the file and calls the tested helper. Both normal loading and recovery share that function.

The growth test deterministically exercises the post-inspection read through the same helper; it does not introduce a race hook into the public loader. No claim of a stable file identity between canonicalization/metadata/open, atomic content snapshot, or filesystem I/O timeout. `actual` in TooLarge is now explicitly an observed count when reading stops at cap+1.
