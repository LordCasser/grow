# Design
在 token 创建后立即构造 clone().drop_guard()，dispatcher future 的正常退出、abort 或 panic unwind 都会释放它。恢复任务只持有 token 克隆，不持有该守卫，避免取消源寿命被子任务延长。正常 cancel/drain 继续执行以等待标记释放，abort 时仅保证通知而不声称同步 join。

实际 run_dispatcher 处理 TransportClosed，经 MockActions 通知已取得去重标记后 abort 并 await dispatcher；虚拟时钟推进至退避后，验证无 respawn 且标记释放。生产 fatal/超时 abort 路径在 actor/teardown.rs 已核对。
