# Tasks

- [x] 核对当前活动成员、稳定身份、`GroupSpan` / `EntryId` / `expanded_groups` 与 dense truncation 的调用路径，明确 live/replay 共享的分组依据。
- [x] 实现连续工具与 Thought 活动聚合、整组摘要及折叠/展开行为，保留 Skill 独立行和真实工具类型。
- [x] 覆盖组边界、纯 Thought、失败成员、运行到终态、待响应/Hook、显式成员展开和工具类型细化场景。
- [x] 验证 live/replay 同序、追加和布局 resize 下的分组及手动展开状态稳定，并验证 `show_thinking_blocks` 与 dense truncation 语义。
- [x] 更新开发者说明，链接归档行为契约及布局排查入口。
- [x] 运行相关定向验证和 `openspec validate --all --strict --no-interactive`，记录场景核对结果。
