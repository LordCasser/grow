# Initial pinned-toolchain findings

Initial full snapshot commit: `567560b82f5e7d2f1420877f4364bf3ebaaefbcf`; worktree clean before tests. All existing pending source/test/doc/archive changes and 2.5.0 metadata are included.

Rust 1.93.1 default CLI and workspace all-targets checks pass. All-targets emits six test-only warnings: one unused mutable GrowStdioClient binding, five noop clones of `&Effect`. Independent read-only review confirms removing these preserves receiver/borrow and assertion semantics. Production code unchanged.

First core run stops at `chat-state::timeline::architecture_document_names_current_timeline_schema`: the architecture header already states domain v27, but its rewritten first line lacks the exact `> Current Timeline schema is v27.` prefix required by the existing guard. Restore that explicit prefix while retaining session v7 and physical record v1. Do not weaken/change the test or schema.

Record first failed run and rerun the full core matrix and all-targets check after committing these documentation/test-only corrections. This is a validation discovery, not evidence of a runtime storage failure. Python generated `__pycache__` is removed; subsequent commands use PYTHONDONTWRITEBYTECODE=1. Runtime/test state remains under independent HOME/GROW_HOME; no application/model variables were inherited.

Second core run passes the schema guard but finds the Dashboard lazy-child-transcript regression fixture still writes a retired `updates.jsonl` plus an empty Timeline. The production reader deliberately ignores the retired file, so the fixture delivers no tool row. Migrate only this test setup to the existing `write_timeline_history_fixture` helper, which uses the production strict journal codec; assert no updates file exists. Preserve the one-tool-row and child-replayed assertions and Dashboard dispatch path. This validates the new storage contract instead of reintroducing compatibility or weakening the scenario.
