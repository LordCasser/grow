# Verification

- `cargo fmt --all -- --check` — passed.
- `openspec validate --all --strict --no-interactive` — passed; 16 items validated.
- With `PAGER_BINARY="$PWD/target/debug/grow"`, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_PROFILE_TEST_SPLIT_DEBUGINFO=off`, and `CARGO_BUILD_JOBS=4`:
  - `cargo test --locked -p pager --test pty_e2e_minimal minimal::minimal_parked_plan_commits_to_scrollback::minimal_parked_plan_commits_to_scrollback -- --ignored --exact --test-threads=1` — passed; 1 test.
  - `cargo test --locked -p pager --test pty_e2e_minimal minimal::minimal_parked_plan_survives_quit::minimal_parked_plan_survives_quit -- --ignored --exact --test-threads=1` — passed; 1 test.
  - `cargo test --locked -p pager --test pty_e2e_smoke plan_revise_empty_enter_does_not_approve::plan_revise_empty_enter_does_not_approve -- --ignored --exact --test-threads=1` — passed; 1 test. The fixture now initializes its isolated Git project, seeds mock LLM config, and waits for the agent prompt before submitting `/plan`.

The initial revision fixture attempt used `action="amend"` and was rejected by the runtime. Inspection confirmed that request-changes returns an unapproved Plan to Drafting; the revised candidate therefore uses `submit`. The amended fixture passed after this correction.
