# Why
MCP 恢复调度先取得 in-flight 去重标记，RAII 守卫却在 spawned future 首次 poll 时才构造。LocalSet 在任务首次运行前销毁时，future 被丢弃但标记没有对应守卫释放，破坏恢复生命周期的成对约束。

# What Changes
在成功取得标记后立即构造守卫并移入 future，覆盖 stdio 和 HTTP。

# Impact
仅恢复任务的资源所有权和回归，不改变重试策略、时间或传输。
