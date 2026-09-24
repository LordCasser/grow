# Design

Reuse the existing `TranscriptFileWrite { agent_id, session_id }` identity; no new job token or registry is necessary. A small lookup in `dispatch/ctx.rs` resolves this exact identity against root views and their child views. For a present session ID, use the existing session view lookup but recheck both the Agent ID and the view's current session ID: a child map key can remain the old session key after its view is rebound. The `None` case remains scoped to a top-level view with the same ID and no session ID. This prevents a stale result from targeting a different view that happens to be active later.

`enqueue_file_write` uses that lookup for the immediate “Saving file…” or full-queue notice. `TaskResult::TranscriptFileWritten` uses it after the queue's existing completion-ID check, and always advances the queue even if the originating view has gone away. The file request keeps the child's captured content, path, Agent ID and session ID from dispatch time.

Tests exercise a Minimal active child with different parent content and cwd, switch to the parent before completion, and verify that only the child receives completion feedback. Separate removal/rebinding cases prove stale feedback is suppressed while the write still completes.
