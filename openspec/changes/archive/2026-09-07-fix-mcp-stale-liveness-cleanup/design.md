# Design
set_liveness_handle 在共享 slot 锁内替换并丢弃旧 handle，旧 token 因 DropGuard 被取消。clear_liveness_slot 在同一锁内检查调用任务 token，取消时返回 false；未取消才 take 自己的 handle。take 后仍在锁外 drop，保留原资源释放方式。TransportClosed 分支仅在清理成功时发送事件，Transient 静默退出。

确定性回归直接模拟状态检查完成后遇到 slot 替换：装旧 handle、替换为新 handle，旧清理不得取消新 token；新清理仍可正常释放槽位。再运行实际 poller 和 servers 的 liveness 测试。
