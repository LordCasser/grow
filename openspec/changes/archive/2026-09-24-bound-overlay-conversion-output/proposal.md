# Bound overlay conversion output before it grows

## Why

Kitty overlay conversion already rejects source headers above 16 million pixels, gives `sips` a 10-second process deadline, and refuses to read an output file above 100 MB. The `sips` child can still fill its private output file before Grow checks its size. The Rust PNG fallback writes to an unbounded `Vec`, so its output allocation is also decided only after encoding.

## What Changes

- Limit the `sips` child process's output file size to 100,000,000 bytes at launch on Unix, while retaining its existing process-group deadline and private workspace cleanup.
- Encode the Rust fallback through a writer that refuses bytes beyond the same output limit instead of growing an unbounded vector.
- On either limit, return no converted overlay bytes and preserve the existing preview failure behavior; do not alter source admission or direct PNG transmission.
