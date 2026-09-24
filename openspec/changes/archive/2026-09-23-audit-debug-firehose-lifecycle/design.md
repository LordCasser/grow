# 设计

以真实非阻塞 file writer 做短时、临时目录内测量。并发首开现有测试证明同一 sink 只保留一个 worker；新增不同 session 测试逐个建立 sink，记录 guard 数按 session 数线性增长，文件字节等于写入行字节总和，并在 flush 后确认 guard 数归零。子进程隔离避免清空进程级 guard 影响其他测试。

Retention 的代码入口是 per-session `install_firehose` 完成 subscriber 初始化后的一次 `sweep_old_logs`；没有 timer 或字节配额。已有跨进程测试验证协作锁持有时旧文件保留、释放锁后可清理。新增 Unix 对照覆盖未获取共享锁的旧文件：清理器可获取独占锁并 unlink，即使另一个非协作 FD 仍打开。mtime 判定先于 lock，但获锁后复查，避免把活跃的协作文件误删。

不提出自动淘汰 worker 的修复：`NonBlocking` writer 与 parked `WorkerGuard` 共同持有 sink 生命周期，当前架构没有按 session 安全关闭 writer 的接口；强行淘汰会引入静默丢日志风险。
