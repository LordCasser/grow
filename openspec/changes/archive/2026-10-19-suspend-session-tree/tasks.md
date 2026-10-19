## 1. Proposal and contract review

- [x] 1.1 完成 proposal/design/delta/acceptance 的一致性和对抗性检查，记录静态证据、既有契约冲突及仅文档交付范围；以 verification.md 为证据。
- [x] 1.2 执行本 change 与 `openspec validate --all --strict --no-interactive`，记录真实结果；不将格式通过视为实现完成。全量校验存在另一 change 的范围外失败，见 verification.md。

## 2. Durable suspension protocol — completed

- [x] 2.1 增加 typed 树挂起/恢复事实、节点续跑引用和校验，按仓库策略同步 schema；checkpoint 保存严格 64 位十六进制 runtime source hash，并由 Timeline 验证；CommandsSettled 按 operation 单次提交且支持同内容 ACK 重试；用 Timeline fold/serde 测试验证同身份冲突、非法 frontier、无效 source hash、缺失/冲突命令结算、终态竞争、重复请求、root/child 原 Turn 保留与版本拒绝。验证记录：2026-10-19 ChatState108 567 passed/0 failed/2 ignored；见 verification.md。该勾选仅覆盖本任务的 schema/fold/serde 及其 crate 测试，不代表运行时树协议或 S-01～S-30 已通过。
- [x] 2.2 实现 root/递归 child 准入关闭和 checkpoint 确认，复用现有 owner；用受控 ACK 测试覆盖 spawn 竞争、部分挂起、写入失败和五秒共享期限（S-03～S-07）。
- [x] 2.3 将 sampler/工具/后台命令中止与逻辑任务取消分开，保存 Interrupted/OutcomeUnknown 和 attempt settlement；通过命令进程组、MCP 响应丢失、provider 托管副作用、重试额度/期限及用量 ACK 故障夹具验证 S-08～S-13。
- [x] 2.4 增加 coordinator/runner 的挂起交接及资源退役分支，避免 Drop/cancel_all_children 误终结任务；验证无 child cancelled terminal、无输入 dismissal、完整 writer drain（S-14、S-24）。

## 3. Same-identity automatic restoration — completed

- [x] 3.1 在 cold load 普通 interruption repair 前核对挂起树；增加原 child 加载与 coordinator/active_child_sessions rebind；验证嵌套树、相同 task/session ID 和无重复 Spawned（S-01、S-02）。
- [x] 3.2 恢复 logical turn、待执行工具批次、AwaitChild 与交付回执；通过原 call/receipt 身份核对 S-09、S-15～S-17 的单次结果、单次用量与先恢复关联后执行。
- [x] 3.3 实现幂等恢复接纳及全树就绪屏障；验证双客户端、ACK 丢失、恢复后再崩溃、节点缺失、旧 writer 存活和配置漂移（S-06、S-07、S-18～S-20）。
- [x] 3.4 保留权限/人工问题并重新核对当前授权，不复用旧 permit 或重跑未知 Hook；使用权限请求版本与 transport generation 测试验证 S-21。

## 4. Session integrations and exit UX — completed

- [x] 4.1 保留输入 FIFO、消费/steer 归属，正确释放临时 edit hold；复用输入接纳测试验证 S-22，无重复 Hook、Submitted 或用户消息。
- [x] 4.2 将 Goal/Workflow 纳入挂起准入及恢复，不改变业务暂停和预算状态；用 Goal usage/Workflow journal 测试验证 S-23、S-25、S-26，pending child 不重复派发。
- [x] 4.3 接入全局 Ctrl+C、正常 Quit 和可控 OS 信号，保留局部复制/只读退出及显式取消/强退；Pager action 测试与 PTY 验证 S-27～S-29，重复按键不调用 force exit。
- [x] 4.4 为挂起资源加入现有 GC/TTL/worktree 删除核对，阻止历史副本带走执行资格；通过清理、fork/import/rewind 测试验证 S-30。

## 5. Integration verification and archive — completed

- [x] 5.1 用真实 Grow 进程、隔离 home/workspace、本地 mock provider 和可计数工具验证主要 Ctrl+C→新进程 cold resume/tree 流程；storage ACK、owner race、超时和失败边界由真实生产 Actor/coordinator 配合受控 persistence/network fixture 验证。按 acceptance.md 逐项记录证据和未覆盖范围，不要求每个故障排列都由 PTY 覆盖，也不把 helper 单测算作真实进程结果。
- [x] 5.2 执行受影响 crate 的必要单元/集成、格式与编译检查；按照 docs/development.md 串行有界构建，记录环境和失败，清理本次 Rust 编译残留并检查磁盘用量。
- [x] 5.3 更新 docs/development.md、架构机制说明和用户快捷键说明，链接行为规范；检查说明没有第二份状态权威或与 delta 冲突。
- [x] 5.4 逐项核对场景和验证记录后运行全量严格校验、`openspec archive suspend-session-tree --yes`，再执行全量及 `openspec validate --archived --no-interactive`；仅在实现已完成时归档。

最终运行证据及分层验收范围见 [verification.md](verification.md) 文首；已完成归档；归档后全量与 archive 校验记录见版本联合验证。
