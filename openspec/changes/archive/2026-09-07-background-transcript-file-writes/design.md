## Current evidence
- root/dispatch/transcript.rs 直接调用 export_cmd::write_export_file 和 clipboard::write_text_to_copy_file，二者均同步执行 write/sync/persist。
- root/effects/mod.rs::execute 接收 JoinSet<TaskResult>；现有 LoadImageViewer 等 Effect 使用 spawn_blocking，结果返回 dispatcher。
- root/dispatch/task_result.rs 的 WorkflowsListLoaded/SessionAgentNameResolved 已按 agent_id + session_id 验证接收方。完成反馈应沿用这种归属检查。
- RenderBlock 的渲染对象不满足 Send；/transcript Minimal 路径已有相关说明。文件任务只携带已生成的 String，不转移 scrollback 对象。

## Decision
AppView 管理一条串行文件写入队列，最多一个正在执行的 Effect。两个命令共用队列，避免同路径跨命令乱序覆盖。每个请求保留 agent_id、session_id、解析后的绝对目标、内容快照和 Copy/Export 策略；Copy 保持 Unix 0600，Export 保留旧权限。

队列按请求数量设有限容量，入队前检查满载并返回提示；不声称单个大文稿的内存有界。容量为 MAX_PENDING=8（不含一个运行项），并测试边界，不提供配置面。请求提交后不随界面切换取消文件写入；但原视图已销毁或绑定不同 session 时，不给新会话追加完成提示。任务结果即使过期也必须推进队列。

TaskResult 携带文件结果，dispatcher 先完成当前队列项，再启动下一项；成功只在原子提交完成后报告，错误保留旧内容并按原反馈显示。后台任务 panic/JoinError 转成失败，不能永久卡住队列。只准一个写入运行，队列可见的待处理请求不能独立 spawn。

## Separate scope and limits
正文渲染仍在 UI 线程；默认剪贴板备份仍同步。该变更只移走显式文件 I/O，不能宣称整个导出操作零阻塞。进程退出时已运行 OS 写入不能强制中止，仍依靠原子提交保证文件完整；不设计磁盘任务持久化或跨进程队列。

## Verification plan
1. Dispatcher 返回写入 Effect 时目标尚未改变；执行 Effect、派发 TaskResult 后内容与成功反馈正确。
2. 挂起首个写入，提交第二个同路径请求，证明第二个不启动；完成后按顺序提交，最终为第二次快照。
3. 切换 session/移除 agent 后返回旧结果，证明不污染新视图且队列继续推进。
4. 写入错误、JoinError、队列满均明确处理；不改变旧文件。
5. 迁移已有 copy/export 的文件回归到实际 Effect 执行闭环，保留路径、权限、加载和选择断言，不能仅删除旧即时写入预期。
