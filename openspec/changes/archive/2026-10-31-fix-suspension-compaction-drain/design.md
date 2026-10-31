## Context
runtime_suspend 关闭后 handle_prompt 在终态结算前 park，等待 suspension owner 丢弃任务。suspend_node 的活动 compaction 分支却等待任务 stopped，等待对象比必须保护的压缩 owner 生命周期更长。

## Decisions
先运行失败回归。复用现有 CompactionLease，增加释放通知及异步 idle 等待；用原共享 deadline 等待 lease 排空，随后 abort 并 join foreground。lease 原子状态同时保留挂起准入关闭位，guard 释放保留该位；挂起先关闭再取消，恢复执行时重开，避免已获 SessionActivityPermit 的手动 worker 在等待背景排空之后取得新 lease。lease guard 必须覆盖原 Sideband/Goal settlement；不提前 abort 未确认 ACK 的压缩。

## Risks
测试在摘要请求进行中挂起，再控制压缩 Failed 的 ACK；Summary ACK 会阻塞 ChatState 后续命令，不能用它启动挂起。测试不模拟真实磁盘或断电。挂起准入已关闭；普通前台不能在 lease 排空后启动下一采样。保留未确认 owner 时仍返回错误，不提交 Suspended。
