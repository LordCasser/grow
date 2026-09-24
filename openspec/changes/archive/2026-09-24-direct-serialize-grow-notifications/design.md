# 设计

两个出口均以 `serde_json::value::to_raw_value(&notification)` 编码 `GrowSessionNotification`，省去 `to_value` 创建的中间树。缓冲路径和普通路径仍分别沿用现有 gateway 转发方式；任何序列化失败仍静默跳过通知。

本次是纯实现重构，不改变协议字段或客户端可观察行为，故 `.openspec.yaml` 设置 `skip_specs: true`。
