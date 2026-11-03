# Verification

Cargo commands used `GROW_HOME=/tmp/grow-home-hunk-tables` and `RUST_MIN_STACK=16777216`.

## Before-change baseline

The before benchmark ran before changing production code:

```sh
GROW_HOME=/tmp/grow-home-hunk-tables RUST_MIN_STACK=16777216 cargo test --locked -p hunk-tracker --lib diff::tests::file_hunk_patch_baseline_microbenchmark -- --ignored --exact --nocapture
```

The output is `/tmp/grow-backlog-file-line-tables-before.txt`; `/tmp/grow-backlog-file-line-tables-baseline-path.txt` points to it. The pre-change actor state coverage also passed:

```sh
GROW_HOME=/tmp/grow-home-hunk-tables RUST_MIN_STACK=16777216 cargo test --locked -p hunk-tracker --lib test_get_file_hunk_data_
```

Result: 6 passed. Its log is `/tmp/grow-backlog-file-line-tables-actor-before.txt`.

## Byte parity and after measurement

```sh
GROW_HOME=/tmp/grow-home-hunk-tables RUST_MIN_STACK=16777216 cargo test --locked -p hunk-tracker --lib generate_hunk_patch
GROW_HOME=/tmp/grow-home-hunk-tables RUST_MIN_STACK=16777216 cargo test --locked -p hunk-tracker --lib diff::tests::file_hunk_patch_reuse_microbenchmark -- --ignored --exact --nocapture
```

The byte-oracle command passed all four matching formatter tests, including CRLF, Unicode, absent final newline, empty sides and multiple hunks. The after microbenchmark passed its per-patch equality checks before timing; raw samples are in `/tmp/grow-backlog-file-line-tables-after.txt`. The old and new kernels were timed on the same fixture in one process with two warmups and seven raw samples. Fixture construction and patch equality checks were outside timing.

## Actor boundary and full crate suite

```sh
GROW_HOME=/tmp/grow-home-hunk-tables RUST_MIN_STACK=16777216 cargo test --locked -p hunk-tracker --lib test_get_file_hunk_data_
GROW_HOME=/tmp/grow-home-hunk-tables RUST_MIN_STACK=16777216 cargo test --locked -p hunk-tracker --lib
```

The actor query filter passed all 6 tests covering Full, Binary, TooLarge, Missing path, new file and mixed Full/Binary states. Added assertions compare exact actor patch bytes for Full/Full, Missing baseline and Missing current. The full library suite passed: **200 passed, 0 failed, 1 ignored**.

`rustfmt --edition 2024` was run on `diff.rs`, `actor/queries.rs` and `actor/tests.rs`; `git diff --check` passed. The temporary timing tests were removed from the build. Their source and fixture are preserved in `probe.rs`. Independent production review passed.

主代理strict全量22/22通过。归档时任务已被拆成单独“归档”项，机械勾选未匹配，archive --yes完成但首次archived校验报告该1项未勾；归档完成后据实际操作勾选该项，重跑archived校验，结果见下。没有缺失业务验证。

最终 `openspec validate --archived --strict --no-interactive`：792 passed/0 failed。
