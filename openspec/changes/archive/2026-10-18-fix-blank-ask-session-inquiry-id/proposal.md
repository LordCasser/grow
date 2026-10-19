## Why

本机 session `01a14f53-fdcc-77d2-8dc5-48cde3252075` 的 timeline 显示三次 `ask_session` 调用实际都传入 `inquiry_id: ""`。模型回复声称省略了该字段，但工具说明只是建议省略，实际参数仍为空字符串；当前 `AskSessionInput` 只有 `None` 会触发 backend 生成 UUIDv7，`Some("")` 会进入 runtime 并因 `inquiryId must be a UUIDv7` 被拒绝。

## What Changes

- 在模型工具 `AskSessionTool::run` 入口将空白或仅空白的 `inquiry_id` 按未提供处理，并更新参数说明，明确 omit、null 和 blank 的语义。
- 保留非空字符串原样校验：有效 UUIDv7 可供重试复用，非法非空值（包括带周围空格的值）和非 UUIDv7 值继续拒绝。
- 保持外部协议/ACP 显式 ID 与 `get_inquiry` 的严格校验；不改变其他会话或实际询问行为。

## Capabilities

### Modified Capabilities

- `local-coordination`：明确模型工具入口的空白 ID 生成语义及外部协议边界。

## Impact

影响 `AskSessionTool::run` 模型工具入口、工具参数说明、相关回归测试与开发者说明。只规范模型工具输入；协议调用方仍须提供有效的 UUIDv7。
