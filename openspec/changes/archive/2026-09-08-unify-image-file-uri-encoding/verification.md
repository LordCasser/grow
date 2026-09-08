## Final verification
Production URI writes in shared orphan recovery and Pager append_prompt_images use file_uri_from_path. Retained legacy pager-render builder writes were updated at the same protocol boundary; none of its unrelated budget behavior was changed. Client-support replaces its unused direct urlencoding edge with the already workspace-pinned url dependency; all runs used --locked --offline, no library version upgrade.

- `cargo test --locked --offline -p client-support --lib placeholder --quiet`: 54 passed (0.01s). Real temporary files cover space, literal%20, literal%2F, #, ?, Unicode, and dedup with space/literal-percent files simultaneously present. Non-file/query/fragment cases are rejected; relative producer path gives no optional URI.
- First non-UTF8 test attempted to create an invalid-byte filename and failed with EILSEQ on this filesystem before URI code ran. The final test checks Unix path-byte conversion without creating that unsupported file; %FF and the original PathBuf round-trip through the absent-file fallback. This is not a real invalid-filename filesystem round-trip claim.
- `cargo test --locked --offline -p pager-render --lib prompt_images --quiet`: 156 passed (0.09s), including retained builder and path parsing coverage.
- `cargo test --locked --offline -p pager --lib image_loader --quiet`: 4 passed (0.01s); the live payload equality fixture now uses `saved %20 #? 图片.png` and decodes the generated URI back to the canonical file.
- git diff --check clean, strict all16 passed. Existing Pager compact-unwind warning, exit0.

No real clipboard operation or live UI send was run; Windows file-path behavior was not cross-tested. Old ambiguous unescaped percent URIs are not guessed by file existence; all producers now emit unambiguous encoding. Other non-image file:// uses were out of scope.
