# 重载后恢复自动 recap 的空闲时间

## Why

真实图片 focus 旅程发现：cold load 的 actor 将 last_api_request_at 初始化为 0，自动 recap 的 idle gate 要求该值非 0。即使历史有三轮已完成的主请求、离开超过三分钟，自动 recap 也永远无法进入 provider。

## What Changes

Existing Timeline bootstrap 从已验证的主 Request 事实恢复最近 API 活动时间；不从 Sideband、UI 活动或 load 时间伪造时钟。Fresh 与新 seeded task 保持没有当前会话 API 证据的原规则。保留三分钟、三轮、新 turn、away identity 与取消门禁。新增核心 HTTP/Timeline 回归并重跑 Full/Minimal 真实焦点端到端验证。
