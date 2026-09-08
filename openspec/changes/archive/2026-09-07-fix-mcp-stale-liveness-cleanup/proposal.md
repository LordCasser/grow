# Why
旧 liveness watcher 在 tick 分支 await 状态检查时，外部可以替换句柄并取消旧 token。旧任务随后仍无条件清空共享 slot，可能取走新 watcher 并取消它，使恢复后的连接失去健康监测。

# What Changes
清理槽位时在同一锁内检查旧 token 是否已取消；被替换的旧任务退出，不清理新槽位，也不发送该次关闭事件。

# Impact
仅 liveness 退出清理；复用现有取消身份与 mutex，不调整轮询频率或恢复策略。
