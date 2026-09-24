# Verification

## Contract and implementation

`minimal_api::selected_child_key` is the single Minimal selector: it yields the child only while the root permission queue is empty and the exact child key exists. `app_visible_agent(_mut)` feeds native commit, viewport sizing, live drawing, plan and expansion; the shared Minimal input router drops a missing child key before dispatch. The owner snapshot includes root and child Agent/session identities, so a rebound child starts a new visible epoch. `try_begin_visible_epoch` advances that marker and resets only the selected view's native frontier after the visible screen clear is accepted. Terminal history from prior epochs is retained.

The print-once contract now applies within each visible epoch. The pre-existing commit frontier still marks entries only after a successful native write, and a failed write remains in the live tail for retry. Plan insertion remembers the last plan per exact view so a switch back reprints the retained block without appending another copy to the conversation.

## Evidence

- `cargo check --locked --offline -p pager -p pager-minimal` passed.
- `cargo test --locked --offline -p pager --lib minimal_` passed 74 tests. The new root tests use distinct parent/child prompt and history content, switch back, verify per-view plan/expand state, root permission precedence, missing child input fallback, and session rebind invalidation. The rebind assertion was strengthened afterward and passed again as a focused test.
- `cargo test --locked --offline -p pager-minimal --lib` passed 94 tests. The new cross-crate tests verify selected-child tail stamping, viewport/native-frontier agreement, failed-commit retry, and that failed terminal clear leaves the old owner/frontier unchanged until a successful retry.
- `cargo build --locked --offline -p cli --bin grow` passed. The ignored real-PTY `minimal_commits_response_to_scrollback` test passed after its fixture seeded a mock LLM config and isolated Git project; it confirms native terminal history still receives committed output. Its first run stopped at the pre-existing missing-config startup gate, not at rendering.
- `cargo fmt --all`, `git diff --check` and `openspec validate align-minimal-child-view-ownership --strict --no-interactive` passed.
- `openspec validate --all --strict --no-interactive` passed 16/16 items before archive.
- After archive, strict active/current validation passed 15/15 and archived validation passed 430/430; `git diff --check` passed.

The child-switch assertions operate on the real AppView and Minimal renderer seams. The PTY check exercises native terminal output for a root view; no automated PTY fixture currently opens a child Agent. The atomic clear branch is covered at the owner-transition seam with an injected failure rather than an OS terminal fault.
