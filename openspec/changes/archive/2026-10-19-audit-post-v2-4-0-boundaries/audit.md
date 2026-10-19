# v2.4.0 之后的边界审计

审查基线为 `v2.4.0`（`cd5d88ec7f5d5c42ee18277304c352232dcb4e14`），终点为 `e7b422990830eee87d68051ed541ecb42f34dffe`。范围包含六个提交、206 个变更文件；文件数包含规范、测试及历史记录，不代表逐个文件做了同等深度的验证。

本轮确认两个新增的图片恢复投影缺陷，以及一个 v2.4.0 已存在的 Plan 持久化缺陷。另有一项较低优先级的 CI 触发覆盖缺口。仅记录问题与复现材料，不实施修复。执行结果和验证限制见 [verification.md](verification.md)。

## F1 · P2 · Behavior 切换的 durable 快照丢失 Plan 候选与提交计数

**归属：既有缺陷。** v2.4.0 已使用 `BehaviorSnapshot::selected(mode)` 构造切换快照；本轮 completed Plan 修改只在部分路径补回 completed_plan 和计数，没有消除内存状态与持久状态的分歧。

触发顺序：

1. 提交 Plan，选择保留稍后审阅，当前内存和 Control 都有 Deferred 候选。
2. 手动切换到 Normal，按既有规则再次选择完成确认，Behavior 切换成功并确认其 Control 写入。
3. 在后续完整 Control 快照写入前异常退出，再从持久会话恢复。

`session_mode.rs:1222-1229` 从空白选择快照开始，只在已有 completed_plan 且不清理它时恢复两个字段。Deferred 候选未被复制，`last_plan_submission_id` 回到 0；而同次切换随后调用 `BehaviorCoordinator::select_behavior`，只清空 PlanRuntime 和 pending switch，继续保留候选和计数。于是运行中仍能看到候选，latest Control 已经丢失它。完成计划后手动选择其他 Behavior 也会把 durable 提交计数清零。

冷恢复使用最新 Control，不会自动回退到更早的候选。因此不能把“成功切换”当作完整状态已持久保留。计数回退还破坏跨 Plan 周期提交身份的单调性；本轮没有证明旧批准一定会被接纳，不把它描述为批准绕过。

**影响边界：** 正常挂起会在 `suspension.rs:1232-1237` 重新捕获内存状态并持久化，能修复这段窗口。此问题针对异常中断或尚未被正确快照覆盖的 latest Control，不是每次正常退出都会丢计划。

代码链：

- `crates/codegen/shell/src/session/actor/session_mode.rs:1222`：构造并持久化切换目标。
- `crates/codegen/shell/src/session/behavior.rs:266`、`:518`：快照默认值与实际内存变更不一致。
- `crates/codegen/shell/src/session/actor/goal_support.rs`：`persist_behavior_transition_durably` 将传入 Behavior 保存到 Control。
- `crates/codegen/shell/src/session/control.rs`、`storage/jsonl/mod.rs`、`actor/spawn.rs`：选择 latest Control 并恢复 coordinator。

契约：`behavior-goal` 的 **Retained plans remain under the existing session owner**、**Later plan approval binds the reviewed submission**。

最小修复方向：以现有 coordinator 的真实切换结果构造目标快照，保留独立于 PlanRuntime 的会话字段；继续先持久确认再应用内存变化，不引入第二个状态 owner。验收应覆盖 Deferred → Normal/Goal/Workflow、完成后切换、在切换 ACK 后直接读取最新 Control，以及从该快照恢复后新提交身份不复用。

## F2 · P2 · cursor 重连把同一次图片恢复拆成两行

**归属：新增回归。** ImageRecoveryOperation 行在 `404aaa79` 引入。

触发顺序：旧 scrollback 已收到 Started；`begin_session_reload` 暂存旧 scrollback 并创建 fresh continuation；同一 operation 的 Completed 在 load 完成前抵达；服务端 cursor 命中，不需要完整历史重放。

`apply_image_recovery_operation` 只在当前 fresh scrollback 查 operation_id，找不到旧行便创建第二行。重连成功时 `append_entries_from` 合并旧历史与 tail，只处理 coordination 和部分 immutable event 去重，没有按图片 operation identity 合并。结果同一 operation 同时有 Started 和 Completed 两行。如果前台 prompt 仍活跃，`settle_stale_image_recovery` 会恢复旧 Started 行的 running 标记；图片已经转换完毕，旧行仍显示进行中。

代码链：

- `crates/codegen/pager/src/app/acp_handler/session_notification.rs:1667`：查找范围只包含当前 scrollback。
- `crates/codegen/pager/src/app/agent_view/session.rs:642`、`:877`：stash/fresh tail 与 cursor 成功合并。
- `crates/codegen/pager/src/scrollback/state/mod.rs:468`、`:513`：缺少 ImageRecovery identity 合并。
- `crates/codegen/pager/src/app/acp_handler/session_notification.rs:1727`：按活跃 prompt 重新挂入 running index。

契约：`client-surfaces` 的 **Image recovery is one visible operation row**，尤其是 cancelled/replayed 和 resident reconnect 场景。

最小修复方向：让 cursor tail 合并继续维护同一 operation 的唯一行及终态，不能仅在同一个 scrollback 内去重。验收须包含 Started → begin reload → Completed → cursor success 的交错，以及进度、失败、取消和终态后迟到进度。

## F3 · P2 · 子 Agent 的图片恢复事件未投影

**归属：新增功能漏接。** 同一新通知只接入 root handler。

当子 Agent 的文本模型需要 vision/OCR 恢复图片时，后端仍可执行恢复，但 Pager 子视图没有对应操作行。child 分流白名单缺少 `ImageRecoveryOperation`，`handle_child_session_notification` 也没有该分支，最终返回 false。这不是样式问题，子任务等待图片转换及其成功/失败结果都不可见。

代码：`crates/codegen/pager/src/app/acp_handler/session_notification.rs:594`、`:1784`、`:1893`。root 在同文件的 ImageRecoveryOperation 分支调用共享投影函数，child 未复用它。

契约：同一个 **Image recovery is one visible operation row**；通知的会话归属不应让普通 child view 丢失已经送达的 lifecycle。

最小修复方向：将该通知按 child session 路由到其 scrollback，复用已有 operation 投影与终态收尾；同时验证 child live、history replay、cursor reconnect。只添加一个处理分支而不核对重连合并，不能同时解决 F2。

## F4 · P3 · Windows CI 的路径过滤遗漏构建配置

**归属：新增 main push 门禁的覆盖缺口；PR 的同类遗漏更早存在。**

`.github/workflows/windows-session-storage.yml:21-31` 新增的 main push 只匹配存储代码、锁文件和少数测试路径。仅修改 `.cargo/config.toml` 或根 `Cargo.toml` 不会触发该工作流；前者当前直接定义 Windows 的 `+crt-static` 等 rustflags，属于会影响这些测试构建的输入。

这会推迟 Windows 回归反馈，但**不会绕过发布门禁**：release 经 `workflow_call` 传入验证后的 SHA，无条件执行 Windows storage，并在 publish 的依赖和结果表达式中要求成功。本轮未运行 hosted Windows job，不将静态触发审查表述为远端验证结果。

最小处理方向：将直接控制该测试构建的 workspace 配置纳入 PR/main path 集合，保持两个触发器一致；不扩展为全仓任意路径都运行。对应 CI 源码已由独立 Luna 审查核对。

## 核对范围与未扩大事项

| 范围 | 本轮重点 | 结论边界 |
| --- | --- | --- |
| Plan 状态、保留报告、详情和 Agent 切换 | coordinator → durable Control → UI/query → restore；TODO runtime 的持有者 | F1 是既有持久化分歧；未把 UI 局部状态当作恢复权威 |
| 图片 sideband 与 projection | operation identity、重连、child 分流、终态收尾、离线 snapshot | F2/F3；已有顺序更新测试不能代替重连交错测试 |
| Skill 读取与发布 | 相对路径、canonical root、UTF-8、输出预算、取消、Timeline 参数和权限路径 | 未确认额外新缺陷；不把未证明的文件竞争当作权限漏洞 |
| Sampling、Goal 与挂起 | 失败归属、已接纳辅助活动排空、deadline、writer/checkpoint 顺序 | 未确认额外新缺陷；没有重新跑真实 provider 或长会话性能基准 |
| 导出、trajectory 与 CI | 临时文件生命周期、same-SHA reusable workflows、exact test 失败判据 | F4 是提前反馈缺口，release barrier 仍在 |

已有 backlog 的同步 compaction TODO 清理、通用工具错误被后续 Step 遮蔽、child 历史重复复制/重放成本继续独立处理，不重复计算为新问题。MCP 多轮 elicitation 的现有提案未实现，本轮不扩展其范围。并行的 release warning 清理也不属于本次审计。

Atlas 用于定位局部结构，索引非全仓完整且部分位置陈旧；最终证据以固定 HEAD 的原始源码、差异和实际执行的探针为准。本报告不是全仓正确性证明。
