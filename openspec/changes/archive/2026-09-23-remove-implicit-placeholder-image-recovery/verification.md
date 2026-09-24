## Evidence

- Repository-wide Rust symbol search found no production caller of `load_placeholder_image*`, `load_canonical_placeholder_image`, `recover_orphan_placeholders*`, `extract_placeholders` or the path allowlist helpers outside `client-support/src/placeholder_images.rs`. Pager and Shell call `strip_paths_from_image_placeholders`; Pager constructs explicit attachment URIs with `file_uri_from_path`.
- Removed the unused file-opening and orphan-recovery APIs, rather than retaining an allowlist-to-open pathname race in an exported module. Preserved text-anchor sanitation and file URI helpers. Sanitization now processes every matching anchor; a 20-anchor regression test covers the former 16-item limit.

## Validation

- `cargo test --locked --offline -p client-support placeholder_images --lib`: 10 passed, 0 failed.
- `cargo check --locked --offline -p shell -p pager`: passed.
- `cargo fmt --check -p client-support`: passed.
- `git diff --check`: passed.
- `openspec validate remove-implicit-placeholder-image-recovery --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: 16 passed, 0 failed before archive.
