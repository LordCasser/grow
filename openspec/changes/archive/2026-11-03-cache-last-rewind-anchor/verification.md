# Verification

Environment for Cargo commands: `GROW_HOME=/tmp/grow-home-c3`, `RUST_MIN_STACK=16777216`.

## Targeted behavior checks

```sh
GROW_HOME=/tmp/grow-home-c3 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib prompt_cache_key_matches_reverse_scan_for_rewind_positions
GROW_HOME=/tmp/grow-home-c3 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib prompt_coordinate_rewinds_and_reuses_branch_indices
GROW_HOME=/tmp/grow-home-c3 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib prompt_cache_key_tracks_timeline_model_and_rewind_lineage
```

All three commands passed. After removing the temporary ignored benchmark from the build, the cache-key differential command was run again and passed on the final Rust source.

## Chat-state library suite

```sh
GROW_HOME=/tmp/grow-home-c3 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib
```

Result: **604 passed, 0 failed, 4 ignored**, 608 discovered. At the time of this full run, one ignored test was the temporary C3 reverse-scan-versus-index microbenchmark. It was explicitly run separately and passed; its full source and samples are preserved in `probe.rs` and `measurement.md`. The other three ignored tests were already ignored. The benchmark test was then removed from the build, and the final cache-key differential test was rerun successfully; production code did not change after the full suite.

## Static and OpenSpec checks

```sh
git diff --check -- crates/codegen/chat-state/src/timeline.rs crates/codegen/chat-state/src/actor/request_builder.rs openspec/changes/cache-last-rewind-anchor
openspec validate cache-last-rewind-anchor --type change --strict --no-interactive
```

Both passed. Production diff also passed independent review.

主代理归档：全量strict21/21（含三个下一增量的已建提案）；归档命令完成后，`openspec validate --archived --strict --no-interactive` 791/791通过。归档提示proposal缺标准二级标题，已补Why/What Changes/Impact，内容不变。一次误用`--archives`被CLI拒绝，随后按help改用`--archived`并取得以上通过结果。
