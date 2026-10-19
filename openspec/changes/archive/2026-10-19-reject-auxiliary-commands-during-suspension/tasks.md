## Tasks

- [x] 核对用户 panic、会话挂起前缀、当前候选及现有活动准入/收尾机制。
- [x] 在修改生产逻辑前复现 late AISuggest 的真实 mailbox panic。
- [x] 最小修复有限活动命令的关闭处理和挂起接纳边界，保留结算与恢复入口。
- [x] 验证迟到命令、worker 首次运行前关闭准入/阻塞 owner、deadline 未确认及后续排空；确保无额外模型请求或伪 checkpoint。
- [x] 执行关联库、跨层/PTY、格式和 OpenSpec 验证，记录结果与证据限制。
