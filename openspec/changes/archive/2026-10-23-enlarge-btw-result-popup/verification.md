## Verification

- `cargo check --locked -p pager` passed.
- `cargo check --locked -p pager-minimal` passed.
- Pager unit tests passed for popup geometry, the complete-frame active/saved popup comparison and unchanged scrollback height, saved `/btw` double-click, archived Markdown viewer construction, and popup hit priority over a covered status badge.
- Existing `/btw` focus, dismissal, scrolling, Markdown rendering, and link tests passed in the filtered run up to `minimal_does_not_scroll_unpainted_btw_geometry`. The filtered run stopped there without a result; the test exercises the separate minimal input path. This does not affect the targeted fullscreen verification above.
- `rustfmt --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive` passed (17 items).
- `openspec archive enlarge-btw-result-popup --yes` applied the client-surfaces delta and created the archive.
- After archival, `openspec validate --all --strict --no-interactive` passed (16 items) and `openspec validate --archived --strict --no-interactive` passed (706 items).

The minimal surface was checked for compilation and left on its existing native-scrollback design. No interactive terminal session was used for visual inspection.
