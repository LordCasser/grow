## 1. Reproduce and establish control result

- [x] 1.1 写 Goal Active 下已接纳但未运行消息的编辑／撤回回归：覆盖队列面板键盘与鼠标、当前 turn 与 Goal 均保持运行；确认 Normal 的同类路径不回退。
- [x] 1.2 写 Shell 竞态回归：hold 与队首提升同时发生时只能有一个胜出；已提升项的 hold／撤回应明确失败；编辑中的队首不得合并或提升。
- [x] 1.3 将服务端队列 hold、编辑、撤回从无结果通知改为可确认的控制结果；携带 id、版本及编辑身份，确认 UI 不会在结果到达前声称成功。

## 2. Queue hold and user operations

- [x] 2.1 在现有 `step_control_gate`／idle arbiter 内阻止持有队首被提升；保持 FIFO、notification、Goal 的既有优先级，不增加 Goal 专用队列。
- [x] 2.2 保存编辑时完成新 input 的 durable admission，再原子替换旧 identity 并解除 hold、唤醒仲裁；失败时保留原输入、hold 与编辑稿。
- [x] 2.3 实现放弃编辑、撤回、断开及过期结果的 hold 清理；版本不符／已经运行时给出明确失败，不能删掉活跃 turn。
- [x] 2.4 Pager 根据 Shell 确认进入或退出受保护编辑态；Goal Active 的直接编辑／撤回和 Normal 共用同一队列操作，且反馈最终状态。

## 3. Verification and closure

- [x] 3.1 验证保存后的消息只在下一次正常 idle 发出点以新内容执行一次；放弃编辑保留原文，撤回后不执行，持有队首不让后项或 Goal 续跑越过。
- [x] 3.2 验证多客户端冲突、客户端断连／重连、optimistic echo、持久化失败与已有队列合并行为；记录实际命令与结果到本 change 的 `verification.md`。
- [x] 3.3 更新 `docs/architecture/input-routing.md` 的队列编辑 hold 解释；运行相关测试、`git diff --check`、`openspec validate --all --strict --no-interactive`，通过后勾选任务、归档并复验规范。
