## Why

Goal 详情的 token 数字没有分隔，普通会话状态栏没有累计消耗和缓存命中率。实际会话的 26 次 `get_goal` 均在 70–340 ms 内成功，但 ACP 转换漏掉 GetGoal/CreateGoal 完成事件，界面继续计时直到 turn 收尾；其中 23 次在 step 0，续跑上下文和工具说明需要明确无需例行查询。

## What Changes

- 补齐 GetGoal/CreateGoal 的 ACP 完成投影，让已有工具行立即结束计时；保留 Session 的查询与生命周期所有权。
- Goal 行为提示、续跑指令和工具说明明确使用已有上下文，按需查询，不例行轮询状态或 token。
- Goal 详情完整 token 数字复用千分位格式；Usage 已有格式保持一致。
- 没有 Goal 的 Agent 状态栏在相同位置显示会话累计 token 和输入缓存命中率，点击打开 Usage 页。使用账本变化事件推送，不增加定时查询。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `behavior-goal`：已有上下文下的按需查询指令。
- `client-surfaces`：完整 token 数字格式和普通会话状态栏的用量投影。

## Impact

涉及 tools 的 Goal 说明、shell 的 Goal 提示与 ACP 通知、chat-state 账本事件、Pager 状态栏和 Goal 详情。复用现有账本、事件通道、Usage 面板和格式化函数，不改计费公式、持久化 schema、预算或子 Agent 权限。

当前工作树还有 `unify-sampling-attempt-recovery` 的并发实施。本次不改父子通信或 attempt 结算；普通账本漏记内部失败 attempt 的已知边界由该 change 管理。真实会话的采样与方法见 session-analysis.md；不把 UI 行存活时间当成后端查询耗时，不将尚未证实影响此问题的主命令队列风险混入实现。
