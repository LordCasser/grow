父 Timeline 的 Spawn 已记录精确的 prompt、child_cwd、worktree_path。Shell 在发布可交互 child 时持有同一事实，随 Spawned 附上传输用展示详情。普通新建和 fork 继承的模型 Surface 没有初始展示行；恢复委派及重建的通知仍须按原规则重放。历史展示事实无需再存一份任务提示；回放使用既有批量、经校验的所有权投影。

Pager 在建立路由时直接使用通知详情。仅对确定在首次输入前发布、且无需继承展示的实时启动，将子 transcript 置为已初始化。正在回放的父会话、历史通知以及资料不全的通知保留原有 child replay 语义。

`resume_from` 在同一冻结读取上检查父 Spawn/Ended、授权边界及来源 child 的完整 Timeline 和 Result。阻塞 worker 持有该读取，Shell 仅异步等待结果；失败仍先于派生会话准入。未改发布回执、超时、控制发布与首个 prompt 的先后顺序。
