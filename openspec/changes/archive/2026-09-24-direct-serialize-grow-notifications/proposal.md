# 直接序列化 Grow 通知

## Why

Grow 扩展通知在缓冲与普通转发路径中都先序列化为 `serde_json::Value`，再转成 `RawValue`，产生可避免的中间 JSON 树分配。

## What Changes

两条路径直接从 `GrowSessionNotification` 序列化为 `RawValue`。序列化类型、扩展方法名、传输时机与错误处理保持不变，因此不改变行为契约，不新增 delta spec。
