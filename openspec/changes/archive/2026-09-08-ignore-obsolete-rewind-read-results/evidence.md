# Evidence

- `views/rewind.rs::handle_input` allows Esc in Loading and Previewing; Executing consumes input.
- Four FetchRewindPoints producers and one RewindPreview producer exist in the rewind dispatcher. All now capture a unique read ID with the issuing session.
- The effect runner echoes read IDs through every transport, decode and success result branch. No request JSON fields changed.
- Previously points-loaded rebuilt state after dismissal; points-failed discarded the stashed draft. Preview success/failure similarly rebuilt state without ownership checks.
- The shared task-result gate filters only the four read result variants. Execute success/failure continue unguarded through existing reconciliation.
