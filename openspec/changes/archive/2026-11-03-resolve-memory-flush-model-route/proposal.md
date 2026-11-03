# Memory Flush 显式模型使用完整路由

## Why

flush_model 只覆盖请求模型字符串，仍沿用主 provider 的 endpoint、backend 和认证，可能把另一 provider 的别名发给主 provider。复用已有辅助 sampler resolver 解析完整配置，确保图片能力、请求、持久 route 和 transport 是同一个实际模型。

## What Changes

仅修复本次图片旅程涉及的 Memory Flush；AI suggest 等其它指定模型入口仍单独处理。
