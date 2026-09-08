# Design

## 已核对证据
servers.rs 客户端状态锁生产入口包括 reset_transport、recover_from、ensure_initialized 的接纳/结果提交、is_healthy、liveness_check、state_kind、server_instructions。握手位于状态锁外。ensure_initialized 的 Initializing 分支显式 drop guard 后等待 Notify；改同步锁时应进一步通过词法作用域确保 guard 不跨 await，保持 Future Send。

## 实施边界
仅 ClientState 锁使用 parking_lot，不调整 McpState 等其他 tokio Mutex。InitGuard::drop 等待短时同步锁后按当前 Initializing 状态恢复目标；无后台任务，不会把迟到清理排到新握手之后。所有临界区必须不含 IO 等待或 await，并核对替换旧值的 Drop 行为。

## 回归迁移
新增另一个线程持锁时 Drop 不能提前完成、释放锁后恢复 Empty/Pending 的回归。旧 initialization_cancelled_while_committing 测试制造异步提交锁等待，该窗口随同步锁消失，不能保留同线程持锁再 poll 的死锁写法；应改为真实线程竞争。concurrent_recoveries_admit_one_reset_and_share_handshake 应以受控握手保证两个恢复共享同一初始化，不能删除共享服务及握手次数断言。

## 实施结果
ClientState 已改为 parking_lot::Mutex，ensure_initialized 将接纳临界区放入独立词法作用域后再等待 Notify。异步调用接口保持原有调用形态，但私有锁不再提供异步排队点。守卫直接同步取得锁恢复。

实际初始化取消回归改为另一线程销毁握手 future，主线程暂持状态锁；旧提交锁等待不再存在。并发恢复测试先 poll 两个恢复，验证仅一次 reset 加一次握手的 revision 递增，最终服务 Arc 相同且初始化总数为 2（含初始握手）。

SafeTokioChildProcess::drop 同步发进程组终止信号，再调度 child.kill/reap；RunningService::drop 由 DropGuard 取消任务，不等待服务退出。状态临界区不执行异步 join。
