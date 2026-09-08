# Design
使用 Option<ClientState> 替代 Option<PendingTransport> 保存守卫取消目标，不引入新的并行布尔状态。初始化构造 Some(Pending(restorable)) 或 Some(Empty)；disarm 取 None；drop 在现有状态锁可用且仍为 Initializing 时安装目标并唤醒等待者。

真实 stdio 使用 /bin/sleep 子进程保持无响应，首次 poll ensure_initialized 后销毁 future，验证 Empty、通知，以及下次初始化立即报无 transport。测试仅 Unix；启动路径保持 kill_on_drop。锁竞争下确定性清理仍独立处理。
