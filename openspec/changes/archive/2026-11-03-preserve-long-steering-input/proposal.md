## Why

运行中粘贴长文本会进入 followup/steer 路由。现场会话的原始 payload 完整保存，但共享 format_interjection 将模型可见正文硬截到 25000 字节附近，丢掉中后部设计和最终用户指导；模型因此要求用户重发剩余部分。此路径没有普通 prompt 的 offload 全文入口。

## What Changes

- 共享插话格式化只添加中途消息说明及 user_query 包裹，不按固定字节数删除已接纳正文。
- 继续通过既有安全消费边界、原输入身份、Timeline 和模型上下文预算准入，不另建 artifact 或输入 owner。
- 独立修复同一现场的交付验收问题，见 `expose-delivery-review-bindings`。

## Capabilities

### Modified Capabilities
- `input-admission`: 长插话的消费正文保真。

## Impact

修改 tools 的共享插话 formatter、Shell drain 注释和回归，不改普通 prompt offload、Timeline schema、排队语义或工具批次安全边界。
