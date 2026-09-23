## Why

会话 `01a0ae68-474d-73b1-94fd-878274dec03b` 的 Timeline 第 2621 行包含 sampler 正常写出的 `sampling_evidence/recovery_stop`，但读取端只接受 request、response、retry，导致恢复误报 `invalid sampling evidence reference`。这与现有 session-timeline 保留恢复决定证据的契约不一致。

## What Changes

- 采样证据读取接受已有生产者写出的 recovery_stop，覆盖会话加载和导入导出的共用校验入口。
- 保留未知类型、记录名称不一致和损坏 artifact 的拒绝行为。
- 增加恢复和证据传输回归，并核对用户会话；不删除、重写或伪造历史，不改变重试策略。

## Capabilities

### Modified Capabilities

- `session-timeline`: 明确停止恢复证据在会话读取与导入导出中的有效性。

## Impact

仅修改 shell 的 sampling evidence 解码允许类型及相关测试、开发说明；不新增存储格式、依赖或状态所有者。
