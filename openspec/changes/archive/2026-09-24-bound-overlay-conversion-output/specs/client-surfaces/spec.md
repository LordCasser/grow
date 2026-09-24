## ADDED Requirements

### Requirement: Kitty overlay conversion bounds created output
On Unix, the spawned `sips` converter SHALL inherit a 100,000,000-byte process file-size limit before it writes output. The Rust PNG fallback SHALL accept at most 100,000,000 encoded result bytes, including an exact-fit result. Converter failure at either limit SHALL produce no converted preview bytes and release owned temporary files. These limits SHALL NOT be described as a bound on source decode memory or direct terminal rendering.

#### Scenario: Converter writes beyond its artifact limit
- **WHEN** the `sips` child attempts to grow an output file past 100,000,000 bytes
- **THEN** its write fails or the child terminates without creating a larger file, and Grow rejects and cleans up the conversion.

#### Scenario: Rust encoder crosses its output limit
- **WHEN** the PNG encoder writes an exact-limit result or attempts one more byte
- **THEN** the exact-limit result is accepted, while overflow fails without extending the result buffer or returning a converted preview.
