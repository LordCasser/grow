## Why
交互式 /copy 和 /export 虽已使用原子文件提交，仍在 dispatcher 中执行目录读取、文件写入和 sync_all，慢文件系统会阻塞事件循环。后台化必须同时保持用户提交顺序和会话归属，不能仅将每次写入独立 spawn_blocking。

## What Changes
对两个命令的显式文件输出引入应用内串行任务队列，快照内容和已解析目标后通过 Effect/TaskResult 执行后台写入。完成提示绑定原 agent/session；队列满时明确拒绝，不丢弃已有任务。复用当前两种原子写入策略。

## Capabilities
### Modified Capabilities
- client-surfaces: 会话文件输出的后台执行与归属。

## Impact
Pager actions/effects/task_result、AppView 的文件写入队列和两个 dispatcher。CLI 同步命令、剪贴板路由/默认备份和正文渲染不在本变更后台化范围内。
