# 挂起与压缩排空审计

## 已复现的互等

`AgentTask` 的 stopped sender 由实际任务持有，任务结束或被 abort 后 receiver 才就绪。挂起关闭 runtime_suspend，再取消压缩。原 suspend_node 检测 lease 活跃时直接等 stopped；同步压缩的取消会完成 Sideband/Goal settlement、记录压缩 Failed 并释放 lease，但 handle_prompt 在最终 Turn 结算前进入 park_during_runtime_handoff，等待挂起 owner 丢弃任务。双方等待对象构成循环，直至共享 deadline。

真实 handle_prompt 回归让摘要 HTTP 请求先进入受控等待，再启动挂起；截住压缩 Failed ACK 验证仍持有 lease 和 foreground，放行后原实现报 `active compaction owner did not settle before the suspension deadline`。它没有使用 running_task_stub 的 stopped 模拟退出，因此覆盖了前台 handoff 的实际执行路径。

初版把 Summary ACK 截在挂起开始前，会阻塞 request_node_suspension 的 ChatState 查询，不能复现目标交错；最终夹具改在挂起取消后的 Failed ACK 控制顺序，不把初版夹具超时作为生产证据。ACK 持有期间也不调用 ChatState 查询来判断 checkpoint。

## 修复边界

CompactionLease guard 覆盖整个 run_compact_only / 发布函数，包括压缩事务终态及 Sideband/Goal settlement；释放比整个 handle_prompt 生命周期更短，正是挂起必须保护的边界。先 await lease idle，再 abort 和 join foreground，避免截断在途写入或等待 park 自退。

Notify waiter 在读取 owner 前 enable；guard 先释放原子状态再广播，覆盖多个等待者和释放竞态。idle 状态可直接返回。

仅 wait_idle 仍允许迟到 acquisition：manual worker 可已拿到 SessionActivityPermit，却在 cancel_background_compaction_before 等待中；挂起发出的 cancel 在 scope 尚未建立时无效。lease 用同一个原子状态携带 closed 位；挂起先 close，再 cancel，try_enter 只接受完全开放的 idle；guard drop 保留 closed 位，恢复执行时 reopen。这样关闭与 owner 取得由同一个 CAS 状态协调，没有第二份 owner 账本。

背景生成本身不持 publication lease，仍由后续 cancel_background_compaction_before 沿原 deadline join；session_activities 不能替代 publication lease，因为同步自动压缩不持 SessionActivityPermit。

本修复不将挂起转成逻辑 Turn 终态；回归检查没有 TurnEnded、没有后继 provider 请求，并保留 deadline 后同 operation 重试。
