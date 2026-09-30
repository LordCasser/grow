## Change

Use the repository's `cargo fmt --all` output as the only source of edits. Inspect the diff to confirm that it only reorders Pager module declarations and wraps one test constructor expression. The changes must not alter the declarations or call arguments.

## Verification

Run `cargo fmt --all -- --check`, `git diff --check`, and strict OpenSpec validation.
