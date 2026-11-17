## Design

Observe the current tracker through the same resident session handle validated for session/load. Reuse WorkflowUpdated and the existing task-state-only replay metadata. Route each projection to the requesting client and acknowledge it before load completes. Before returning, recheck that the same actor is still registered and its command channel is open. Pager uses the existing Workflow revisions and clear tombstones when applying the current state to its task model. Ordinary live and historical Workflow updates retain their existing transcript handling.
