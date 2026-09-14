# Design

Capture `session_id` and `session_binding_epoch` when dispatching
`ShowContextInfo`, propagate both through the effect worker into successful and
failed `TaskResult` variants, and reject results whose AgentView identity or
epoch no longer matches. The guard runs before `apply_full_context_info`,
scrollback insertion, or modal mutation. `nonce == 0` remains the minimal
scrollback intent; nonzero nonce remains the usage modal fetch epoch.

