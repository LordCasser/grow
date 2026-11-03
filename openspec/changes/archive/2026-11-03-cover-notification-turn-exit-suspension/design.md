## Context

`attached_as_viewer` 属于前端逐回合的 prompt 归属门禁，不是 SessionActor、child 或 writer 的所有权。宿主创建的 notification prompt 不在客户端的 self-originated 集合中；真实 ACP chunk 因而使该标志变为 true。旧 `begin_exit_suspension` 在没有 non-viewer 页时直接发出 Quit，错误跳过 runtime 挂起请求。

事故中 parent 在 12:22:44.247 启动 `origin=subagent_completed` 的 internal Turn，prompt 为 `notifications-01a19fa5-2ab6-7a72-a2fc-82f909e7e9e4`；12:24:28 shutdown 取消的是同一 Turn。该证据比“进程启动早于修复”更直接地对应错误 guard。

## Decisions

复用现有 dispatch fixture。先记录一个客户端原始 prompt，经过真实 ACP handler 接收不同的宿主 prompt chunk，确认逐回合 viewer 标志与当前 prompt 均按生产逻辑变化；再从 AppView 接受两次 Ctrl+D，核对只产生 SuspendForExit。挂起成功 ACK 前不能 Quit 或 CancelTurn；ACK 后才退出。

反证实验只恢复旧函数开头的 non-viewer guard，不回退整个文件或覆盖已有工作区修改。同一回归应在旧 guard 下失败，恢复当前实现后通过。现有递归树 PTY 结果仍用于验证真正的 child 挂起/恢复，不把本次 UI 测试称为独立模型进程端到端测试。

## Evidence boundary

旧进程没有持久保存 viewer 布尔值或分支决策；可以把实际通知 Turn 与确定性的旧源码机制对应，但不能声称直接读取了退出瞬间的内存。记录这一限制，不用它回避已找到的错误边界。
