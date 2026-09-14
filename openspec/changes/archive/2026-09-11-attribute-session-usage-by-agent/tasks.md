## 1. Ledger attribution

- [x] 1.1 扩展 `UsageLedger`，让主调用和子 Agent fold 同步维护 Agent 分项；用 chat-state 单元测试验证主 Agent、两个子 Agent、多模型与 incomplete 的归属和总体算术。
- [x] 1.2 沿 shell → chat-state 命令与 ACK 链路透传现有 `subagent_id`；用 session fold 测试验证匹配与 session-only 两条路径都保留身份。

## 2. Aggregate projection

- [x] 2.1 保持 `PromptUsage`、headless 和 Pager 展示形状不变；用 usage 扩展测试验证总体包含子 Agent 且序列化结果没有 Agent 分项。

## 3. Validation and archive

- [x] 3.1 运行受影响的 chat-state、shell 与 usage 投影测试，并把命令与结果记录到 `verification.md`。
- [x] 3.2 运行 `openspec validate --all --strict --no-interactive`，核对所有场景后归档 change，再运行 active/archive 全量校验。
