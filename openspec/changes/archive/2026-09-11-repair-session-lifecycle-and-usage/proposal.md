## Why

Pager 在后端 session 已退出时仍会把无返回值的取消通知当作已进入可等待的 `Cancelling`，缺少后续终态便永久锁住输入；强制退出后，失败的 resume 又会留下一个已解绑但仍可见的伪 Agent 页面。与此同时，会话 Usage 只保存当前 actor incarnation 的内存累计，冷 resume 后历史总量和 Agent 归属都会丢失。

## What Changes

- 为 `TurnCancelling` 接入既有权威 prompt-status 对账：取消后若终态通知丢失或 session 已不存在，在短窗口后查询并收敛为终态/空闲，而不是无限等待。
- 失败的首次 session load 删除临时 Agent，并返回发起 resume 前的 Welcome、Agent 或 Dashboard；保留明确错误提示和再次 resume 的入口。
- 将主模型 attempt、子 Agent 结算与 session incomplete 事实投影成可恢复的累计 Usage；冷 resume 不再重置总量。
- 以成功创建的新 actor incarnation 为 resume 分段边界；`/usage` 保留 lifetime 总计，并展示 initial run 与各 resume 段的新增消费。normal 状态栏继续只显示 lifetime 总体，Agent 分项继续只记账、不直接展示。
- **BREAKING**：移除“Usage 只覆盖 start/last resume 之后”的旧语义，`grow/session/usage` 增加分段数据。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`: 取消状态必须可由权威查询收敛；失败 resume 必须离开不可提交的临时 Agent；Usage 界面展示 lifetime 总计与 resume 分段。
- `session-timeline`: session Usage 结算、完整性和 resume 分段边界必须可持久恢复且不可重复计费。

## Impact

影响 `chat-state` 的 Usage 账本与 Timeline 恢复、`shell` 的 session usage/加载路径，以及 `pager` 的取消 watchdog、session-load 失败归位和 Usage modal。ACP prompt usage 与 headless 最终结果不增加 Agent 分项；只有 `grow/session/usage` 的专用响应新增 resume segments。
