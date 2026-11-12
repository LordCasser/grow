## Decisions

Use the validated Timeline history to pair retained background task starts with settlements by task and tool-call identity. Include earlier suspensions in the selected history. Reuse the existing task completion projection and Tasks reducer, keeping an existing visible completion when present. Use recorded start and settlement times and the settlement's result and output artifact. This is a read-only projection. It adds no journal event or model-facing notification.

Run the PTY regression across two processes. Start a background command, verify it is running, suspend with Ctrl+D, and resume the same session. Verify the Tasks row is terminal.
