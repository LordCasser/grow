## Why

v2.1.10 的响应恢复审计发现四个边界问题：重复 rewind 丢失保留响应的 admission 来源；fork 复制父投影但子 Timeline 无其 authority；resident reconnect 的初次 cache snapshot 会吞掉较早 Timeline snapshot 之后完成的响应；exact append 以写后可读替代 durable ACK。证据见 [响应恢复审计](../../review-grow-architecture/reviews/response-recovery-merged-2026-09-17.md)。

## What Changes

- 保留跨重复 rewind 的 response provenance，切走的分支仍排除。
- 普通 fork 从已核对的父回放投影继承普通历史展示，不携带父 admission authority。
- resident 初次回放在遇到 Timeline snapshot 之后的新 projection 时保留物理截点，交由后续 delta 交付。
- exact projection/ACP append 只有完成文件和目录同步后才返回 durable success；重复提交仍去重。
- 为四个故障边界增加定向回归，更新开发说明并记录验证。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `session-timeline`: 重复 rewind 的响应来源，以及 exact replay cache 的持久确认。
- `client-surfaces`: fork 继承历史与 resident snapshot/delta 边界。

## Impact

涉及 ChatState branch fold、Shell fork/storage/replay 和相关测试。延续现有 Timeline authority，不引入第二套 response 状态、旧格式迁移或新的后台任务。用户已授权修复这四项；原 `reconcile-response-replay-projection` change 的其余未完成治理不在本次范围内。
