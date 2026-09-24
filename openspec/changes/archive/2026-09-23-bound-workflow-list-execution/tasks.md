## 1. Bounded Workflow Listing

- [x] 1.1 Add a bounded blocking scan helper and move `grow/workflows/list` discovery off the async executor; verify with focused shell tests.
- [x] 1.2 Add controlled blocked-worker tests proving deadline behavior, permit retention after timeout, and no extra scan start; verify with the exact focused test filter.
- [x] 1.3 Document the request boundary and validate the OpenSpec change before archive; verify with `openspec validate bound-workflow-list-execution --strict --no-interactive` and the focused Rust test.
