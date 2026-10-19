## Why

版本联合验收发现 normal/minimal 挂起恢复后，headless `--continue -p` 在采样 anchor 持久化时报 `ACP eventId conflicts with persisted event`。冷加载只从 Timeline 的展示期待恢复全局事件计数；完整 updates 历史中的较大 ID 要到客户端 replay 才参与恢复，而 headless 使用 noReplay。这个缺口会阻断合法的新一轮。

## What Changes

- 冷加载在发布 actor 或生成新持久事件之前，从已取得的完整物理 ACP/Grow 更新历史及 Timeline 展示期待共同恢复事件序号下界。
- 恢复计数独立于客户端是否 replay、cursor 及 rewind 后是否展示该行；物理历史已占用的身份不被重复使用。
- 保留持久层对同 ID 不同 payload 的冲突拒绝，补齐 noReplay 与真实进程回归。

## Impact

涉及 Session persistence/load、已有 event ID helper 及 session-timeline 契约。不改 provider ID、不改工具执行，不建立新 ID 注册表或兼容协议。
