## Why

用户报告subagent压缩失败：replacement would remove or change an unconsumed skill load。Timeline已有正确的未消费skill因果保护，但当前summary选区只看Surface/完整响应组，不包含消费状态，可能先生成必然被durable replacement拒绝的摘要。该差异不限child，main与child共享同一prepare_compaction。

## What Changes

- ChatState同一Timeline物化快照提供尚未被canonical主响应消费的SkillLoad Surface IDs，复用现有branch provenance判定。
- 共享compaction planner从范围中排除未消费SkillLoad及其真实Model load调用/结果和完整响应组，保留原identity/正文；source budget收缩仅限原安全范围。
- source不足时在summary请求之前按既有no-safe-range路径退出；已消费skill恢复正常压缩资格。保持Timeline最终保护、真实消费和明确turn终结settlement不变。

## Capabilities

### Modified Capabilities

- context-compaction：选区与摘要输入共同保留未消费skill的因果组。

## Impact

chat-state Timeline/materialization/compaction_utils单一owner，Shell薄传参与同契约真实入口回归；无新持久化schema、配置、权限或消费状态。测试独立GROW_HOME，不干扰正常会话。不可删guard、伪造consumed/response admission或调用terminal settle来使压缩通过。
