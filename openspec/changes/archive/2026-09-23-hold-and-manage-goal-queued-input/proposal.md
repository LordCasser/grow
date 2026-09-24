## Why

Goal Active 期间发送的普通消息可以仍处于 Shell 的待执行 FIFO；它尚未进入模型，却难以像 Normal 下那样直接编辑或撤回。当前 TUI 虽绘制了队列项的 `[edit]`、`[cancel]`，Goal 的 Ctrl+C 入口仍指向停止 Goal／turn，且服务端队列操作是无结果的异步通知，不能把“点击了按钮”当作“成功修改了未生效消息”。

另有一项独立的新需求：用户开始重新编辑队列消息后，该项必须暂时停止发送；完成编辑后，按原 FIFO 位置重新等待下一个正常发出点。现有 Pager 本地队首有 drain 阻挡，但 Shell 的 `combine_edit_holds` 只阻止合并，不能阻止队首在 Goal 或 Normal 的 idle 仲裁中被提升。

## What Changes

- Goal Active 下，仍在权威 FIFO 的用户消息可从队列项直接编辑或撤回，不要求停止 Goal、当前 turn 或子 Agent；已开始执行的消息不冒充可撤回队列项。
- 为正在编辑的权威队列项建立经 Shell 确认的临时 hold；hold 期间该项不能合并、提升或发送。完成编辑、放弃编辑或删除后，按结果解除 hold，并由正常仲裁决定下一次发出点。
- 编辑、撤回及 hold 失败必须反馈给用户；不能因无确认通知或乐观隐藏而丢失编辑稿，或声称一条已运行的消息已被撤回。

## Capabilities

- `input-admission`：待执行输入的编辑 hold、FIFO 顺序与持久化替换／撤回边界。
- `client-surfaces`：Goal Active 队列操作入口及权威结果反馈。

## Impact

- Shell：`session/actor/{prompt_queue,notification_drain,run_loop,idle_arbitration}.rs`、队列控制命令及 ACP 扩展入口。
- Pager：队列面板的编辑／删除路由、编辑态、ACP 效果和结果归约。
- 测试：Normal 与 Goal 共用的队列竞态、Goal Active 的键盘／鼠标操作、持久化接纳和多客户端状态同步。
- 非目标：修改 Goal 生命周期、把排队消息当作 Goal 指令、回滚已消费的消息、改变全局 Ctrl+C 的停止语义，或加入新的持久队列系统。
