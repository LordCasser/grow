## Scope

核对 `SessionActor::prepare_compaction` 的来源快照、选区、原文/预算适配/简化三阶段，及 `ShellCompactionSampler` 到 provider 请求、`commit_generated_compaction` 到范围替换的链路；对照 `compaction_utils` 和已有测试。

## Boundaries

本次不修改压缩、recap 或 memory flush。原始截图未包含 Sideband 账本和实际 wire artifact，不能判断用户的历史会话是否触发过简化阶段。运行纯函数测试用于核对输入变换，不声称完成 provider overflow 的端到端故障注入。
