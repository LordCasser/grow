# Change: Harden cancelled subagent resume lifecycle

## Why

Session `01a0c7c4-70eb-7890-8d17-ddfbde5061cc` 在 root Goal 因 provider 额度错误暂停时取消了仍在运行的异模型子 Agent。父、子 Timeline 后来均包含 `Spawned → Ended(cancelled) → SubagentResult(cancelled)` 的精确关联事实，子 Timeline 也能独立校验和导出，但两次 resume 仍立即失败并统一报告 `no completed canonical lifecycle was found`。

日志同时记录到取消末尾仍有 sampling attempt 结算尝试写入 child Timeline，而 `SubagentResult` 已先关闭该 Timeline，写入因 `child Timeline is closed by its subagent result fact` 被拒绝。当前取消等待只观察外层 cancellation token，未把 child turn、sampler evidence 和 usage settlement 的确认当作提交 `SubagentResult` 前的必要屏障。

恢复解析又以 `Option` 抹去了 parent/child load、身份、安全边界、Timeline 校验和 result-link 校验的具体失败。调用方因此无法区分“仍在运行”“终态尚未提交”“持久化读取失败”和“canonical link 损坏”，并使用包含 `completed` 的统一文案，误导运行时和诊断者把合法的 `cancelled` outcome 当成不可恢复原因。现有记录只能证明恢复解析当时返回空，无法事后还原被抹去的具体分支。

## What Changes

- 将子 Agent 取消改为有确认的终态流程：停止新 provider admission、取消在途工作，并等待已准入 attempt 的 evidence、usage settlement 和 child turn terminal 越过既有持久化边界后，才允许写入关闭 child Timeline 的 `SubagentResult`。
- 明确 canonical `cancelled` lifecycle 是可恢复来源：只要父 `Spawned`/`Ended`、child summary/seed/result 及 `result_ref` 精确一致，resume 不得以 outcome 不是 `completed` 为由拒绝。
- 将 durable resume source 解析改为结构化结果，保留 active、lifecycle incomplete、storage/load、identity/security、child Timeline 和 result-link 错误；对外错误在不泄露越权 session 信息的前提下给出可操作类别。
- 让 live ownership 优先于 durable source：即使磁盘上已出现 terminal link，只要 coordinator 仍持有原 child，就不得并行启动续接；待其完成收尾后重新解析 canonical source。
- 修复非 worktree source cwd 的失效路径：已删除的历史 cwd 安全回退到当前 parent workspace；仍存在但越界、不是目录或无法验证的路径继续拒绝。隔离 worktree、snapshot、模型 route、effort、transport、上下文窗口和 artifact 不兼容继续 fail closed，并返回各自原因。
- 移除 resume 对当前版本 System head 渲染的伪依赖；恢复只接受 source Timeline 中已经验证的稳定 head。历史 completion output artifact 不参与 resume context，缺失不阻断恢复；真正被 source Surface 引用的 prompt artifact 仍须严格校验。
- 将 derived child 的控制状态发布、Goal 快照安装和首轮 prompt 持久化纳入启动准入：任一步未确认都以明确 launch failure 收尾，不能继续启动 provider，也不能损伤原 source 的再次恢复资格。
- 保证 source 解析之后的新 resume epoch 启动失败不会消耗、改写或伪装原 source；调用方可在修复环境问题后用同一 source 重试。
- 增加确定性交错回归：root Goal 停止触发异模型 child cancellation，阻塞最后一个 attempt settlement，证明 canonical terminal 不会越过该屏障；释放后同一 cancelled child 可被 resume，且恢复继续固定原 child 的模型身份。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `session-timeline`: 明确完整的 cancelled child lifecycle 可作为 durable resume source，并要求恢复拒绝保留可诊断原因。
- `model-sampling`: 将 child cancellation 的 attempt evidence/usage settlement 纳入 `SubagentResult` 之前的终态屏障。

## Impact

- 主要实现范围：`crates/codegen/shell/src/agent/subagent/handle_request.rs`、`crates/codegen/shell/src/agent/subagent/mod.rs`、child session cancellation/turn shutdown 路径，以及相关 subagent/sampler/coordinator 测试。
- `crates/codegen/chat-state/src/timeline.rs` 继续以 `SubagentResult` 作为 child Timeline 的不可逆关闭事实；不放宽现有 seed/result-link 完整性校验。
- 不改变 Goal 暂停与恢复规则、provider 额度分类、模型选择策略、子 Agent 权限、UI 展示或 worktree 语义。
- 不增加新的持久化实体、Timeline event 或迁移；fresh spawn 仍是新任务，不作为 resume 的语义替代。
- 不修改历史损坏记录。已有完整 canonical lifecycle 应由修复后的 resolver 直接识别；不完整 lifecycle 继续 fail closed。
