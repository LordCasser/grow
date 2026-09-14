## Why

会话账本虽然已有子 Agent 终态回传入口，但折叠时丢弃了 `subagent_id`，因此无法证明总量由哪些 Agent 构成，也无法为后续核账区分主 Agent 与各子 Agent。当前 `/usage` 和 normal 状态栏仍应保持简单，只展示全体 Agent 的总体消费。

## What Changes

- 会话用量账本同时维护总体、provider/model 分项和 Agent 分项；主 Agent 与每个子 Agent 的已知消费分别归属。
- 子 Agent 完成后的累计快照沿既有 ACK 边界折叠到父会话总体，并使用原 `subagent_id` 记录归属。
- `/usage`、headless usage 和 normal 状态栏继续只投影既有总体及 provider/model 信息，不新增 Agent 分项 UI 或 wire 字段。
- 保留当前进程窗口、完成后折叠和 incomplete 语义；不新增持久化或运行中轮询。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`：明确会话总体包含已结算子 Agent 消费，内部保留 Agent 归属，而当前 Usage 与 normal 界面只显示总体。

## Impact

影响 `chat-state` 的进程内 `UsageLedger`、shell 的子 Agent 终态用量命令及相关测试。`PromptUsage` 公共形状、Pager 渲染、Timeline 与持久化 schema 不变，不增加依赖。
