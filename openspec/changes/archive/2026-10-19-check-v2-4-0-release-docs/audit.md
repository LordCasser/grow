# 2.4.0 发布前文档一致性核对

## 基线与范围

基线 `d9ebf99c`，本地 main，开始时工作区干净。按 2.4.0 release notes 对照 OpenSpec 主规范、实现及当前用户/开发说明；不将旧版 changelog、带时点的历史审计或未完成 MCP delta 当成现行契约。没有修改运行时代码、主规范、版本、依赖、workflow 或发布说明中的能力。

Atlas project(open) 后做 scoped structural searches，随后直接阅读命中模块及调用方；部分 planning Focus 结果有 coverage gap，未将未命中解释为不存在，以直接源码与规范核对补齐。静态版本/链接检查由 Luna 子 Agent 独立复核。

## 确认并修正的差异

| 主题 | 原描述的问题 | 修正与实现证据 |
| --- | --- | --- |
| Plan artifact | 19-plan-mode 指向已不再使用的 plan.md / approved_plan.md | 改为 artifacts/plan/<hash>.md 与精确 submission/revision/hash；`shell/src/session/behavior.rs::write_plan_artifact/read_plan_artifact`、`actor/tool/preparation.rs` 先持久候选再打开审批。 |
| Plan 延期与收起 | 指南仅说明放弃，未说明 l 延期、r 重审和未确认决定检查 | 补 l / /view-plan / r，区分 Esc/关闭只是收起，q 明确放弃；`pager/src/app/agent_view/viewer.rs`、`plan.rs::defer_plan` 和 `views/plan_approval_view.rs`。不把保留状态当新 Plan 执行阶段。 |
| Plan 的审批边界 | 缺少旧评论/新提交与跨会话历史的权限区别 | 按 `behavior-goal` retained-plan 与 `session-timeline` plan-history requirements，说明旧评论保留为未发送草稿、需要重审；导入拒绝仍带可审阅/未决执行关系的来源，闭合历史保留原支持。 |
| 局部计划所有者 | 17-sessions 把 Todo authority 归 resources_state.json；Goal 说明仍称 short-lived TODO | 改为 SessionActor / Timeline planning authority，Tool Resource 与 ACP 为投影；`shell/src/session/actor/planning.rs::local_planning_context/commit_local_plan`、`tools/.../todo/mod.rs`。GoalState 不复制计划不等于计划不持久化。 |
| 父子弱关联与重规划 | 16-subagents 缺少新参数及实际使用行为 | 核对 TaskToolInput 并将旧 background 参数名改为 run_in_background，补 parent_step_id / required_deliverable、child 自有 TODO、非完成有效修改触发评估、完成/no-op 不触发、命令不被编辑自动终止、终态 child 不复活、逐级传播；`common/tool-types/src/task.rs::TaskToolInput`、`tools/.../task/mod.rs` 的链接接纳与 `common/tool-types/src/planning.rs`。 |
| 结构化等待及交付 | 现行说明缺少等待事件和交付验收边界 | 按 task-planning 主规范补真实依赖 any/all、yield 后仍未完成、保持原 owner/limits、未知失败唤醒决策、必需交付由父级验收、worktree 不自动合入。未新增调度器或“TODO 即权限”说明。 |
| Goal readiness | README/config/slash/架构仍称一空闲就一定采样 | 改为有可推进工作时续跑；全部结构化等待则等事件，另有独立步骤可推进；`behavior-goal` Goal readiness、`actor/goal.rs` 与 `actor/planning.rs`。 |
| Goal architecture | input-routing 当前恢复说明仍写 v9，与 v10 文档冲突 | 对照 `shell/src/session/goal_tracker.rs::GOAL_ARCHITECTURE_VERSION = 10` 修正，保持拒绝旧架构、不迁移的现有边界。 |
| 整树挂起与 Workflow | 会话指南仅列 /quit，slash 文档仍称跨进程 Workflow 一概不能恢复 | 补 TUI Ctrl+D 确认、Ctrl+C 局部/取消、失败重试/显式强退、同一子树身份恢复及中断命令不重放；区分有效 checkpoint 与异常退出。Workflow 按原 Run/journal/child 关系恢复，原用户/业务等待不越过；`session/workflow/manager.rs::restore_suspended_executions/pause_released_suspended_runs_for_manual_resume`、`tracker.rs` 及 workflow-execution 主规范。 |
| Skill 加载 | 用户运行与模型选择、文件读取和加载事实没有明确区分 | 补 runtime 直接 load_skill、用户组原子发布、未确认不放行、原文独立、精确正文快照和挂起/异常恢复；`actor/skill_loading.rs`、`actor/turn/admission.rs`、`extension-runtime` 与 `session-timeline` Skill requirements。 |
| 图片拒绝路由 | input-routing 称负能力按目录 provider/model ID 记录，ModelImageInputKey 只是旁路 provenance | 改为实际 model/backend/endpoint/query route 的稳定 key，同路由 alias 共享，换路由独立，辅助视觉避免同路由递归；`common/tool-types/src/model_image.rs`、`actor/turn/sampling.rs` 与 model-sampling transport-scoped requirement。 |
| MCP 能力与不确定结果 | 指南“任何 MCP 服务”措辞过宽，未说明本版多轮暂缓和不自动重发 | 明确 Form/URL 多轮未开放，补 live catalog/policy、旧调用 deadline 不改及传输丢失/超时不重放；`extension-runtime` 主规范、当前 active MCP change 的 2.4.0 scope decision；没有变更门禁。 |
| 入门权限措辞 | 教程把读取描述为总是免确认 | 与现行 22-permissions-and-safety 的显式 ask/deny/Hook 顺序对齐，改为默认通常免确认，不承诺忽略配置策略。 |
| 标题锚点 | 03 的 WezTerm 链接、08/09 的组织分发链接共三处指向不存在标题 | 修正到实际 Ctrl+Enter does not interject / Check and share it / Trust and security 标题，不为失效链接虚构产品章节。 |

上表源码路径位于 `crates/codegen/`，特别注明 common 的路径除外。当前行为与验收以 `openspec/specs/` 为准，本记录不是新增行为契约。

## 未变更与发布边界

- 当前版本及 2.4.0 notes 标记一致；历史资料中的旧版本按历史保留。
- Ctrl+C / Ctrl+D 的默认路由已经正确，本轮只补操作说明；headless SIGINT 130 是进程信号，不是 TUI 退出键。
- MCP 多轮仍 active 且门禁关闭；Remote Control / Provider cache / Recap 方向的暂缓决定不变。
- 先前 Runtime / PTY 结果见已归档 `2026-10-19-finalize-v2-4-0` 与 `2026-10-19-separate-cancel-from-session-exit` 的 verification，包括首轮 Skill PTY 偶发失败未定位、复测通过的限制；本轮没有重新执行或扩大它们的承诺。
- 旧历史审计的绝对源码 :line 与 /tmp 日志只作追溯，不宣称这些临时证据全部可从仓库取回。
