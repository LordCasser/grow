# Verification

- `cargo test -p shell grow --lib`: 56 passed, including acknowledged actor Grow appends and gateway credits.
- `cargo test -p shell sampling --lib`: 85 passed, including failed preview admission gates.
- `cargo test -p shell projection --lib`: 98 passed, including exact replay and admission behavior.
- `cargo fmt -p shell`: passed.
- `git diff --check`: passed.
