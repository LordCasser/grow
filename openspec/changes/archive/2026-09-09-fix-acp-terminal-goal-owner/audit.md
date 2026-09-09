# Adjacent Goal owner propagation audit

Scope: root / descendant startup, delegated turns, nested Task, runtime Agent replacement, terminal backends, notification conversion and usage settlement. Repository paths below are relative to crates/codegen.

| Boundary | Evidence | Finding |
| --- | --- | --- |
| Root versus child tracker | shell/src/session/actor/goal_support.rs::sync_goal_usage_window and spawn.rs startup | Existing is_subagent guards preserve shared root lifecycle; no remaining production sync writer bypass found. |
| Delegated turn owner | shell/src/session/actor/turn/admission.rs | Earlier 78f13768 supplies missing revision from matching inherited context; strict invalid-owner checks remain. |
| Ordinary Task child | shell/src/agent/subagent/handle_request.rs StartupHints; turn/admission.rs | Window shared for all children but immutable Goal context only for Goal owner; separate fix-task-child-goal-attribution verifies inference respects delegated_goal. |
| Task capture and nested delegation | tools/src/implementations/grow_build/task/mod.rs; shell/src/session/actor/goal_support.rs | Resource reads share one lock; typed owner and context pair are checked; inherited immutable context survives nested delegation. Root task admission also checks current active id/revision against frozen view. |
| Child creation and resume | shell/src/agent/subagent/handle_request.rs | validate_goal_context runs before spawning; SetGoalContextSnapshot precedes QueuePrompt on the same command channel. Current request controls ownership even when conversation source is resumed. |
| Agent/model replacement | shell/src/session/actor/model_switch.rs | apply_prepared_agent copies GoalContextSnapshot, current typed owner, delegation snapshot and mutation authority into candidate bridge. Existing rebuild regression covers preserved owner context. |
| Bash and monitor request assembly | tools/src/implementations/grow_build/bash/mod.rs and monitor/tool.rs | Both owner fields are read from CurrentSubagentOwnerResource under the same resource lock and carried into TerminalRunRequest. |
| Local terminal | tools/src/computer/local/terminal.rs | Request revision retained in process state, task snapshots and completion; no matching omission found. |
| ACP client terminal | shell/src/terminal/adapter.rs | Confirmed omission: TrackedTask retained id only; to_snapshot hardcoded revision None. Fixed independently with adapter lookup/completion regression. |
| Notification conversion and recovery | shell/src/tools/notification_bridge.rs; agent/subagent/mod.rs | Partial owner pairs fail closed; recovered child receipt derives owner from validated durable Spawn. These validators correctly expose upstream omissions and are not weakened. |
| Sideband admission and settlement | shell/src/session/actor/sideband.rs and goal_support.rs | Captured stable Goal id plus attempt identity/owner epoch governs accounting. Definition revision is intentionally not a ledger partition: late usage remains charged after objective edits. Already-admitted attempts do not reread a replacement Goal at settlement. |
| Unowned recovery projection | shell/src/agent/mvp_agent/mod.rs orphan task projection | Explicit None/None pair is a display-only orphan close, not the partial-owner ACP defect. |

The two newly investigated failures are separate from the earlier shared-window and delegated-revision fixes. This audit checks connected ownership boundaries and associated regressions; it does not prove every process/crash interleaving. No provider requests, user-session rewriting or unrelated ownership framework refactor is required.

Final outcome: ACP omission fixed in b915aff0; ordinary Task misattribution reproduced through handle_prompt and fixed in e31dfb09. Full Shell: 3,762 passed, zero failed, 3 ignored. Final OpenSpec strict validation: 17 passed; archive validation: 295 passed. No additional confirmed omission in the inspected boundaries remains open from this pass.
