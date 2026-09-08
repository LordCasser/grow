# Why
握手完成后在 await 状态锁前 disarm，使结果提交等待期间取消丢失恢复责任。

# What Changes
持有状态锁后才 disarm，无 await 间隙再写结果。

# Impact
保持既有 try_lock 取消恢复策略，本次只修复提前 disarm。
