## Context

source SessionActor 是 Timeline durable admission owner；CoordinationHandle 的 outgoing map 是进程内执行与查询投影。backend 目前先 await durable start，再调用 runtime；查询先检查 map，释放锁后读 Timeline。两处均留下错误地观察到“只有 start 没有活记录”的窗口。

原始失败是 `scripts/test_local_coordination.py::is_queued` 收到 `SourceUnavailable`。异常清理会关闭进程并产生 persistence actor unavailable 日志，不能把清理日志反推成触发原因。

## Goals / Non-Goals

目标是让查询线性观察启动接纳，且执行必须在 durable ACK 后发生。冷恢复只读历史、重试去重、取消和未知结果边界保持成立。此次不改变目标 Sideband 执行、模型协议或会话存储。

## Decisions

复用 outgoing map，在验证原绑定后先登记现有 `InquiryRecord`，然后由 detached worker 等待惰性的 actor durable admission future，成功后才进入原派发路径。提交期间查询返回 `Discovering`，同身份重试共享原 record，取消能立即找到它；已有 durable 终态直接复用，不重复接纳。查询在无运行记录时持锁读 durable fact，避免一次查询先错过 map，再跨过另一个启动提交而误读历史。

backend 的现有 owned task 覆盖整个 admission 和 dispatch；丢弃调用方继续通过原取消 token 收束，不以查询触发执行。runtime 现有结果 channel 区分本地 admission 错误与 inquiry outcome；接纳失败通知所有当前等待者、移除失败登记以允许明确重试，不派发，也不追加伪造的完成事实。已有 actor ACK 最长等待 30 秒，等待在 outgoing 锁外进行。没有新增持久化状态、每身份锁或第二份 inquiry store。

不采用仅追加第二次 map 检查，因为第二次检查前仍可能被调度暂停；不将 pending durable fact 一律映射为 live，否则破坏冷恢复 `SourceUnavailable`；不跨 actor ACK 持有全局 outgoing 锁，避免拖住其他查询和取消。不通过接受测试中的 `SourceUnavailable` 或自动重试隐藏错误。

## Risks / Trade-offs

- 接纳 pending 必须保持可取消 → 先登记后等待 ACK，在 ACK 前通过真实 cancel 操作确认成功且目标没有收到派发。
- 测试依赖调度可能偶然通过 → 屏障先确认 start 已落盘、ACK 尚未放行，再在同身份查询；保留现有冷恢复测试。
- 包级测试绕过 backend 边界 → 额外运行实际 CLI 独立进程脚本，并为相关 main push 启用协调 CI。
