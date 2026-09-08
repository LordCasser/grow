# Why
run_dispatcher 仅在事件循环正常结束时取消恢复 token。会话 fatal 或关闭超时路径直接 abort dispatcher，会跳过取消，独立 spawn_local 的恢复任务可能继续重试并触碰正在关闭的会话。

# What Changes
以现有 CancellationToken 的 drop guard 将取消通知绑定 dispatcher future 的生命周期，保留正常关闭的主动 cancel 与 drain。

# Impact
只调整取消源所有权与实际调度器回归，不改变恢复策略或关闭超时。
