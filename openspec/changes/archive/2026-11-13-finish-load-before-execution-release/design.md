# Design

Split the existing suspended-tree restore at the execution-release boundary. Validate, adopt and commit the same checkpointed tree in the existing order. Keep execution closed through the load's control publication, historical reconciliation, foreground snapshot and durable resume accounting. Perform the existing coordinator and leaf-first actor release as the last awaited load operation. The prepared root still owns any checkpointed foreground identity, so the load response can capture it before sampling resumes.

Use the existing tree lifecycle and failure handling. The frontend's full-history input barrier remains tied to the load response. This change does not shorten history loading or suspend already-running resident work when a client reconnects.
