## 1. Reproduce and type resume failures

- [x] 1.1 用事故 session 的 parent/child 导出与日志固定 baseline，并增加 cancelled exact-link fixture，保留原统一失败文案和晚到 attempt persistence failure 作为修复前证据。
- [x] 1.2 将 durable resume source 解析从 `Option` 改为 typed `Result`，覆盖 parent/child load、lifecycle incomplete、security、summary identity、Timeline 和 result-link 类别。
- [x] 1.3 更新调用入口的 active + durable error 映射；验证未授权 lineage 不泄露 session 存在性，且任何失败都不启动 child、worktree 或 provider 副作用。
- [x] 1.4 建立完整 resume failure matrix，并把 policy、source storage/link、identity、route、workspace/context、derived child admission 和 first-prompt 阶段映射到新增或既有回归；合法拒绝保留原约束但不再互相误报。

## 2. Close cancellation only after settlement

- [x] 2.1 调整 active-child cancellation：token 只发起取消，runner 等待 child prompt terminal 或现有 session drain frontier，不再立即合成并持久化 `SubagentResult`。
- [x] 2.2 通过 sampler cancellation、usage ACK、turn settlement 和 child prompt terminal 的确定性边界回归，证明 terminal 不越过已准入 attempt 的 evidence/usage settlement；不新增持久化状态机。
- [x] 2.3 保持 `SubagentResult` 为不可逆 child close；barrier、child result 或 parent terminal 任一失败时 fail closed，不发布成功 receipt、不删除需要保留的 worktree，也不产生可恢复假象。

## 3. Resume cancelled canonical sources

- [x] 3.1 增加 cancelled source 回归，证明 exact Spawned/Ended/Seed/Result link 可恢复，且 resume 继续固定 source agent/model/transport/effort、cwd 与 worktree 规则。
- [x] 3.2 覆盖 active、缺失 terminal、missing child、summary mismatch、invalid Timeline、missing/invalid result ref 与 security mismatch；每类返回预期 typed/error category 并保持原事实不变。
- [x] 3.3 覆盖 completed/failed 既有路径，确认本 change 不把 cancelled 误标为成功，也不改变 fresh spawn 和普通 completion 语义。
- [x] 3.4 将 live pending/active 检查前置到 durable source 接纳之前；核对“terminal link 已可见但旧 runtime 仍在收尾”不会启动重叠 resume epoch。
- [x] 3.5 修复 non-worktree source cwd 的窄回退：missing 使用 parent workspace；existing non-directory、越界、symlink escape 或 canonicalization error 继续拒绝。

## 4. Preserve intentional incompatibility gates

- [x] 4.1 覆盖 source agent type/availability、model catalog、reasoning effort 和 transport key；确认错误明确且不会静默改 agent/model/effort/endpoint。
- [x] 4.2 覆盖 worktree reuse、missing snapshot、rehydration failure/path mismatch，以及 isolation/source-worktree 冲突；失败不修改 source snapshot/result，source 可重试。
- [x] 4.3 覆盖 source Timeline materialization、empty/headless Surface、80% context bound 和 inherited prompt artifact 校验；失败不 fresh-spawn、不隐式压缩或丢历史。
- [x] 4.4 覆盖 parent Spawned、child persistence、session spawn、catalog convergence、cancel-at-promote 和 first-prompt admission 失败；本次 derived epoch 正确收尾且原 source 仍可重新解析。
- [x] 4.5 解除 resume/verbatim fork 对当前 System head renderer 的依赖；确认 inherited head 仍严格存在，并记录 completion output artifact 不属于 resume context authority。
- [x] 4.6 将 control publication、Goal snapshot mailbox、QueuePrompt send 与 durable `persist_ack` 纳入首轮准入；失败时不继续 provider admission，且已可能准入的 prompt 先结算再关闭 child。

## 5. Integrated regression and documentation

- [x] 5.1 组合确定性交错回归覆盖：Goal/coordinator cancellation owner、不同模型 route 固定、阻塞 usage/turn settlement、child prompt terminal barrier、exact cancelled link 与后续 resume projection；不依赖真实 provider 或 sleep 竞争。
- [x] 5.2 更新 `docs/development.md` 的 subagent cancellation/resume 持久化边界，并链接归档后的 `session-timeline` 与 `model-sampling` 契约。
- [x] 5.3 运行受影响 chat-state/sampler/shell 定向测试、必要 package check、changed-file rustfmt、`git diff --check` 和 `openspec validate --all --strict --no-interactive`，将结果写入 `verification.md`。
- [x] 5.4 复核实际 diff 与 cancellation/resume production callers，核对每个 delta 场景和验证记录；归档 change 后再次验证 active 与 archived specs。
