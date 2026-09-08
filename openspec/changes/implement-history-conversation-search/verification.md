# Verification

- Pager session dispatch: 203/203 passed, including initial query, focused search, reopened-picker stale completion rejection and current disabled-index feedback.
- Pager slash registry/commands: 375/375 passed, including Chinese/multiword query forwarding and explicit --prompts recall.
- Pager session picker rendering/selection: 19/19 passed; existing snippet rendering and selection/resume reused.
- Pager history-filtered tests: 53 passed, 1 existing ignored. Includes malformed/error response decoding and existing prompt history request fencing.
- Shell session storage search/FTS/recovery: 73/73 passed against temporary storage; no user session index modified.
- Removed only the unused HistorySearchState.selected stub; selected_text and prompt recall remain active.
- Cargo builds used locked offline dependencies, debug=0, incremental=0 and 2 jobs. git diff --check passed.
