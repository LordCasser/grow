## Why

用户授权将已完成研究落成两个 OpenSpec 提案，并更新 follow 上游的 commit 标记。现有入口仅在 docs/architecture/upstream-borrow-review.md 保留第二轮 eb267fef..bc7f02ed 的历史范围，没有独立机器可读游标。其 MCP“等待 draft/SDK”判断已被2026-09-23归档核查取代。

## What Changes

- 在现有评审入口显式标记已评审目标 `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`，限定为 MCP 多轮和保留计划两项主题，不伪称区间全量提交审计。
- 链接本目录 review 和两个尚未实现的行为 change；旧轮次保留历史语义。
- 在现有 roadmap 权威 openspec/backlog.md 记录 Remote Control / Agent Host：暂缓、未排期、不构成实现授权。
- 记录顶部 task 布局、Dock/Memory/连贯模型选择排除项。

## Impact

仅变更文档导航、评审记录和 backlog，不改任何运行契约或产品代码；因此 skip_specs: true，不虚构 delta，不改主规范。两个独立行为提案保留各自 delta。

不会修改 git refs/remotes/upstream/main、SOURCE_REV 或依赖版本，因为这些不是评审游标，且无 merge/cherry-pick/实现事实。不得以目标 SHA 推断远端最新 HEAD。
