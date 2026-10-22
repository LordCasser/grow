# HookRegistry round-trip 测试样本同步

## Why

整库回归发现现有 HookSpecWire 要求 on_failure，但 round-trip fixture 未包含此字段，反序列化失败。

## What Changes

只把样本同步到既有服务端字段和默认 allow 策略，保留完整 JSON round-trip 相等断言。不改 DTO、默认值或产品行为，skip_specs。
