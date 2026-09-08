# Verification
- main. Deterministic regression with previous path-identity construction failed: writer captured replacement inode rather than open-file inode.
- After descriptor-metadata capture, low-disk `cargo test --locked --offline -p diagnostics --lib unified_log::tests`: 21 passed, 0 failed/ignored. New test covers replacement and unlink between open and construction, then due maintenance and visible append. Existing identity/healing/trimming tests retained.
- Explicit temporary paths only; diagnostics ctor redirects suite logs. No installed binary/live UI, Windows replacement detection, zero-loss-before-next-maintenance or forced metadata-error claim.
