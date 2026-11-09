## Scope

`acp_agent::prompt` 的两个 select 分支都立即 break；删除外层 loop 不改变 future 的 poll、drop 或返回值。biased completion 分支仍先被检查；只有成功 ACK 才发布 Working，ACK drop 后只等待原 completion。后续 admission replay、usage 及 roster 路径不变。

readiness 的权威结论使用已归档验证，不重启已关闭修复。macOS 退役、旧宿主恢复、目标 Chat 接纳性及原 blocking Q&A 各自保留证据门槛，不能用 Linux 或模拟成功替代现场结论。
