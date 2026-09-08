# Why
recover 先通过 state_kind 读 Ready，再调用 reset_transport 重新取得状态锁。两个恢复调用可同时读到 Ready 并分别重置，覆盖另一调用的 Pending/Initializing，违背合并恢复的设计。

# What Changes
在单次状态锁内完成 Ready 判定与 Pending 转换，其余恢复加入已有 ensure_initialized。

# Impact
只修复 recover 的接纳原子性；工具超时后的强制 reset 策略不混入。
