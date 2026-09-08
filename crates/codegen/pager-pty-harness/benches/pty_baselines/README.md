# PTY benchmark baselines

Baselines are per-platform (macOS arm64 has very different timing from an
Linux arm64 CI runner) and per-scenario. The runner compares the current run against the file explicitly supplied
with `--baseline` and fails if any comparable scenario's p99 frame time
grows by more than 15% (default; `--threshold` overrides). It does not select
a platform file automatically.

Use `<platform>.json` to distinguish baselines, for example
`linux-x86_64`, `linux-aarch64`, or `macos-aarch64`.

## Producing a baseline

Run the full bench suite on a quiet machine:

```bash
cargo bench -p pager-pty-harness --bench pty_bench -- \
  --all \
  --write-baseline crates/codegen/pager-pty-harness/benches/pty_baselines/<platform>.json
```

## Overwriting after an intentional perf change

A PR that intentionally shifts frame timing (either direction) must update
the affected baselines. Include the `pty-bench` output from a clean run in
the PR body so reviewers can sanity-check the new numbers.

## First run

The current checked-in `.github/workflows` do not run `pty_bench` or seed
platform baselines. Generate a baseline explicitly with the command above.
`--baseline <missing-file>` returns an error; it does not create the file.
