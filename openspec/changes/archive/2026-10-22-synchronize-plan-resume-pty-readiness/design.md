# Design

复用现代 resume PTY 已使用的 pager.replay.finalize 标记。日志位于本场景独立 ContentController HOME，且只给新恢复进程配置，不可能误读创建会话进程的标记。该 timer 在 SessionLoaded 的同步状态更新结束时 drop，晚于 loading_replay=false；读不到标记则按既有 20 秒预算失败。之后继续通过实际终端发送 /view-plan、Review 和批准，核对 retained body 与 foreground request count，不以诊断替代产品结果。

不使用 sleep、首段/末段历史、模型标签或放宽超时作为输入就绪证明。变更仅在 dev-only harness 中；已构建的 2.5.0 候选可用于本机行为核验，官方发布仍必须重新绑定包含夹具修正的精确提交。
