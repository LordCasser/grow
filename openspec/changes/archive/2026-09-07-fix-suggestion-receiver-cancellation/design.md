## Context
extensions/suggest/ai_provider.rs 与 suggest/mod.rs timeout 丢弃 receiver。当前 actor 始终 await handler 后才尝试 send。SidebandRun 的 Drop 已有取消终态持久化及测试。

## Decision
共享 deliver_suggestion helper 用 biased select，closed 分支优先；拥有生成 future，关闭时丢弃，成功时 send。activity 仍由外层任务持有，helper 返回即释放。不修改超时数值或模型选择。

## Validation
测试关闭前未 poll、执行中关闭导致 Drop、正常交付；已有 sideband drop 取消持久化回归。
