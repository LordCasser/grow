# Design

Inspect the ignored attributes on the named PTY tests and keep backlog claims scoped to those concrete cases. Cargo splits pager PTY tests into `pty_e2e_minimal`, `pty_e2e_config_ui`, `pty_e2e_scroll_selection`, and `pty_e2e_queue` for the listed cases; leader client PTY coverage uses `leader_pty_e2e` and runs serially. Existing unit and neighboring active PTY coverage does not establish that the ignored scenarios pass in the default suite. No implementation or spec delta is needed.

Validation is static: confirm each listed test name and `#[ignore]` marker in its source, review the focused backlog diff, run `git diff --check`, and run strict OpenSpec validation. Do not execute the potentially heavy PTY test targets as part of this documentation change.
