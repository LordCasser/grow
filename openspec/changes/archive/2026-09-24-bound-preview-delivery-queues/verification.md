# Verification

- `cargo test -p shell preview --lib`: 12 passed, including slow gateway exhaustion, held persistence acknowledgement, oversized notification, and gateway failure at the admission barrier.
- `cargo test -p shell sampling --lib`: 85 passed, including exact independent append failure, candidate marker ordering, and sampling admission paths.
- `cargo fmt -p sampler -p shell` completed.
- `openspec validate --all --strict --no-interactive`: 16 items passed before archive, 15 after.
- `openspec validate --all --strict --no-interactive --archived`: 510 items passed after archive.
- `git diff --check`: passed.
