## 审查方法

以持久 session identity、Pager AgentId、actor/writer owner 和 Dashboard row identity 为四个独立观察点，沿用户动作核对状态迁移及失败后的可继续性。

覆盖普通 fork 与 worktree fork，区分 fork 的来源关系和 subagent 的执行/结果引用；检查列表、详情、欢迎页和实际进程退出各入口。对删除拒绝、加载失败、迟到回执和 owner 退出做对抗性核对。

Atlas 用于限定范围的符号/调用发现；索引含已移动文件的历史条目，结论必须回到当前源码确认。执行验证优先使用隔离 fixture，绝不向用户真实会话发送停止、关闭或删除请求。
