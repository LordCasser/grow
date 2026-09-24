The compact response projection now holds `ProjectedText` rather than full ACP notifications. The fork replay memory test converts each serialized text chunk into its corresponding ACP assistant or reasoning update only in its load-all reference path; production replay code is unchanged.

- `cargo check -p shell -p pager --tests`: passed after the test correction.
- `cargo test -p shell --features test-support --test session_fork_replay_memory fork_replay_stream_is_bounded_and_faithful`: 1 passed.
- `openspec validate --all --strict --no-interactive`: 18/18 passed with both in-progress changes present.
- `git diff --check`: passed.
