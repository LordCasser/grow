## Why

用户在 `01a08906-7afd-70e2-af4e-5ff9ea4db84c` 中多次遇到行动预告后立即结束 Turn。原始 Messages SSE 确实返回 `end_turn`，但它只证明一次响应完整；Grow 将“无工具调用”直接升格为任务完成。跨端点的同类表现不能靠中文冒号匹配、重试传输或修复历史投影彻底解决。已有完成要求仅约束少数专用 Agent，Stop hook 默认放行，普通会话缺少显式完成协议。

## What Changes

- 普通模型 Turn 提供宿主控制工具 `FinishTurn`，要求模型明确声明 completed、waiting_for_user 或 waiting_for_background，并给出理由。回答仍使用普通 assistant 文本。
- 无工具响应只结束 Step；缺失或无效完成声明经既有 Step 边界继续，连续协议失败达到上限则报错，不能伪装成完成。
- 完成声明必须独立于业务工具批次，使用真实工具调用/结果持久化；取消、控制切换、预算、拒绝、结构化输出保留各自的终止权威。
- 更新真实三协议 Turn 回归、开发说明和 backlog。修正旧规范将合法 end_turn 直接当作正常 final 的场景。

## Capabilities

### Modified Capabilities
- `model-sampling`: 区分 provider 响应结束和宿主 Turn 完成，并修改普通 final 场景。

## Impact

影响 shell 的模型 Turn 编排、chat-state 的工具结果持久化确认入口及相应测试夹具。没有独立分类器、后台续跑任务、标点词表或配置开关；不修改 sampler 原始终止证据。模型应在最后一条回答同时调用 FinishTurn；遗漏会产生额外一次采样，持续不遵守协议会明确失败。结构化输出已有显式响应契约，不叠加此工具。
