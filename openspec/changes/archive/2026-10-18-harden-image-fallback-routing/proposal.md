## Why

图片拒绝状态当前存入 `ModelImageInputState` 的字符串集合，但 Shell 用 catalog alias 写入和查询。alias 表示配置入口，不保证对应固定运行时：同一 alias 的 endpoint/backend/query route 改变后会错误沿用旧 text-only 拒绝；不同 alias 指向同一运行时则不能共享已确认的拒绝事实。这会分别让新视觉运行时被误跳过，或让已拒绝的运行时重复收到图片请求。

Grow 已有 `ModelImageInputKey`，可表达实际 wire model、backend 和 endpoint/query fingerprint，也已用于图片投影身份。本 change 将其作为 primary、auxiliary 与 compaction 共用的拒绝能力身份。

## What Changes

- 为 `ModelImageInputKey` 提供稳定的 JSON storage key，并继续复用当前 `BTreeSet<String>` resource 形状。
- Shell 对实际已解析 primary/auxiliary transport key 执行拒绝查找和记录；catalog alias 仍用于用户可见模型标签。
- 同 runtime 的 alias 共享拒绝事实；model、backend、endpoint 或 query route 任一不同都形成独立事实。Primary、auxiliary 和 compaction 使用一致的 key。
- 覆盖 alias 复用、同 alias transport 更换、辅助路由重配置、同运行时防递归，以及 projection durable ACK/失败边界。
- 保持原图首次发送、视觉辅助 → OCR → ImageProjection → 请求重建和描述缓存语义；相同描述可跨 auxiliary route 复用。

## Capabilities

### New Capabilities

无；扩展现有 `model-sampling` 图片 fallback 契约。

### Modified Capabilities

- `model-sampling`：新增 transport-bound negative image capability requirement。

## Impact

涉及 `ModelImageInputKey` 的编码、`ModelImageInputState` 的字符串存取边界、Shell 的图片拒绝查找/记录和 compaction 查询，以及共享图片恢复测试。旧 alias 字符串不会作为新 key 命中，不迁移或清除该 resource 中的其他值。测试按 identity、辅助路由和现有投影边界分层覆盖；不要求每种 provider 错误都增加端到端场景。

## Non-goals

不安装 Grow、不修改运行中的 session、不根据模型名称预判能力，也不扩展状态架构或处理其他并发改动。
