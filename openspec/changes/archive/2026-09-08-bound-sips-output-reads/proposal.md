## Why
The sips result is read with fs::read after conversion. Pixel and process limits do not bound the encoded file bytes consumed by Rust, and empty results are currently accepted.
## What Changes
Limit non-empty sips output to100,000,000 encoded bytes. Check an opened regular file and cap actual consumption at limit+1. Reject Unix symlinks/nonregular outputs, using nonblocking open to avoid FIFO replacement blocking. Existing conversion failure fallback and owned-directory cleanup remain.
