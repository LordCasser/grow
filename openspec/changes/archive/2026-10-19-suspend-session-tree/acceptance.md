# Acceptance matrix

本表定义运行验收要求，不是测试通过记录。实施后的实际测试、失败和覆盖限制统一记录在 [verification.md](verification.md)；没有对应运行证据的场景不得视为通过。

基础夹具：隔离 Grow home 与 workspace、本地可控 provider、可记录调用次数/副作用的工具、真实命令进程组、可延迟/丢弃 ACK 的持久层。保存 root/child/task/prompt/call/receipt IDs、provider attempts、工具派发计数、用量账本及退出前后 Timeline，用明确事实判断是否重复执行。

| ID | 场景/注入点 | 必须观察到的结果 |
| --- | --- | --- |
| S-01 | root 等待 A/B，A 完成，B 正在跑命令；真实 Ctrl+C 后进程退出，再由新进程 resume | A 无新执行；B 原 task/session ID 自动继续处理命令中断；父等待绑定原 B；最终两份结果各交付一次 |
| S-02 | B 还有嵌套 child，另有前一 turn 启动的后台 child | 整个 owned subtree 均被挂起/恢复；fork 来源、独立 peer 不被误纳入 |
| S-03 | spawn 在准入前、提交后/ACK 前与挂起竞争 | 未接纳者不启动，已接纳者被纳入；未知身份先核对，无遗漏 child 或重复 spawn |
| S-04 | 各 child 已保存，root 挂起提交 ACK 丢失 | 同一操作重试可核对，成功事实不重复；不生成第二棵树 |
| S-05 | storage full、写入阻塞、节点不可达 | 五秒共享期限内返回未确认，界面响应；无成功挂起标签、无零用量补齐、在途 writer 不被抢占 |
| S-06 | 挂起意图/一个节点 checkpoint/全部节点/root 确认等边界分别杀进程 | 新 owner 逐项核对已提交前缀；缺依据时受阻；有效挂起保留 root/child 原 Turn，无 Recovery terminal、重复 TurnStarted/prompt 或 child cancelled/interrupted terminal，不自动重发未知操作 |
| S-07 | restore 登记前、接纳后但启动前、部分节点已开始后分别崩溃 | 尚未开放执行者核对后续接原恢复；已运行后无新挂起的异常走原中断恢复，不回退或重复消费旧 checkpoint、输入、结果或通知 |
| S-08 | 长时间本地构建、后台命令及其受控后代正在运行 | 快速发送终止并有界确认；确认停止记录 Interrupted/部分副作用可能性；不等待自然构建完成、不将 Agent 写成 cancelled |
| S-09 | 一个工具完成、一个未派发、一个正在运行的批次 | 已完成结果复用，未派发者经权限复核后执行一次，中断者不被 runtime 自动重发 |
| S-10 | 远程写已生效但响应丢失，取消结果不明 | OutcomeUnknown；恢复明确要求核对现场或决定，服务端计数保持一次，不以重连或原调用重试制造第二次写 |
| S-11 | 本地命令的派生进程脱离控制或停止确认失败 | 保留未知/外部可能仍运行，不能声称已终止；Grow 自身不继续派发 |
| S-12 | provider 部分流未接纳时挂起；分别注入 usage/evidence ACK 阻塞、托管工具副作用不明、不可撤销输出、attempt 额度耗尽或绝对期限到期 | 保存中断及已知/incomplete 用量，partial 不成为 accepted response；ACK 受阻不能宣布完整冻结；重新采样遵守原 typed 安全/准入检查，不重置额度/期限，不盲重发 provider 请求 |
| S-13 | response admission 已提交，但 replay projection ACK 未完成或工具未开始 | 恢复先核对/补齐原投影，不重复 provider 请求，保留原 response/call identity |
| S-14 | 节点 checkpoint 完成后运行退出 guard/coordinator Drop | 无取消 child 的尾随消息、无排队输入 SessionClosing dismissal、无新的 SessionEnd 外部 Hook 副作用 |
| S-15 | child 在挂起期间真正完成 | canonical result 保留；不改成挂起、不重跑；完成通知和用量各一次 |
| S-16 | child result 已写，parent Ended/receipt ACK 分别丢失 | 只补原链缺失部分，父只收到一次结果，既有结果正文缺失按 unavailable 报告 |
| S-17 | 原 call 已写 OutcomeUnknown，恢复后取得迟到最终证据 | 原 call 仍仅一份 tool_result，新事实经归属明确的通知/observation 交付 |
| S-18 | 两客户端同时 resume，同一请求 ACK 丢失后再次 resume | 一次恢复接纳、每节点一个 writer/runner，无重复 child 或工具派发 |
| S-19 | 旧 actor 已退但 persistence lease 或 I/O 仍存活 | 新 writer 不接管，显示恢复受阻；旧 owner 完整 drain 后才能继续 |
| S-20 | 任一必要 child artifact 缺失、route/effort 不可用、worktree 不匹配、schema 不支持 | 原树保留，整个树执行准入关闭，精确指出节点和原因，不替换模型或空白 child |
| S-21 | permission/user question 尚未回答；离线期间权限、工具 schema、MCP generation 改变；或 Hook 结果未知 | 原问题身份/内容版本可核对，当前授权重新检查，无旧 permit 复活、无默认 Allow/Reject、无未知 Hook 重跑 |
| S-22 | consumed prompt + FIFO + steer/followup + queue edit hold 同时存在 | 已消费输入不重复，原路由/顺序保留，临时 hold 释放但挂起期间不消费，未提交草稿不被当用户提交 |
| S-23 | Active/Paused/Blocked/BudgetLimited/Complete Goal 分别挂起，混入迟到用量 | 业务状态不被 session 挂起更改；只有合法 Active 可继续，预算不重置，离线不计运行时长 |
| S-24 | 原任务明确 Cancel/Stop Goal 后再挂起，或已在真实 finalization | 已接纳的真实停止/终态优先；resume 不复活；未确认交付责任保留 |
| S-25 | Workflow 正在 await 原 child，挂起后同 Run 恢复 | script/args/route/journal 不变，旧执行结算后换 epoch，pending operation 关联原 child，无第二次 agent() 派发 |
| S-26 | Workflow 原为业务 Paused/AwaitUser/BudgetLimited，或旧 epoch 迟到结果 | 恢复保持原等待，迟到事实按原身份核对，不唤醒退役执行 |
| S-27 | 运行中连续 Ctrl+C、child view 退出、多 root 退出 | 一次全局挂起请求/树，所有将被销毁的 owned root 纳入共同期限，未变为 CancelTurn/force exit |
| S-28 | 有界保存失败后选择重试或明确强退；外部 OS 第一次与第二次信号 | 重试核对原身份；强退明确不保证保存且不写成功标记，terminal best-effort 恢复 |
| S-29 | 局部复制、只读 replay/export、child 历史查看、Welcome 退出及真正 Stop Turn | 不误触挂起执行或自动恢复；真正取消保持取消；正常挂起退出通过原终端 teardown/fence 回归 |
| S-30 | 挂起/恢复受阻期间 TTL、worktree cleanup、fork/import/rewind/delete | 自动清理保留恢复依赖；历史副本不继承运行资格；破坏原恢复引用的显式操作经过现有所有权护栏 |

## Existing verification entry points

- Timeline/Surface：`crates/codegen/chat-state/src/timeline.rs`、`src/actor/tests.rs`。
- Child coordinator：`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs`。
- Child 恢复与控制：`crates/codegen/shell/tests/test_subagent_orphan_reconcile.rs`、`src/agent/subagent/tests/`、`src/agent/mvp_agent/tests.rs`。
- Actor 集成与输入/Goal：`crates/codegen/shell/src/session/actor/tests/`。
- Workflow：`crates/codegen/workflow/src/journal.rs` 与 Shell Workflow 测试。
- Pager 快捷键与真实进程：`crates/codegen/pager/src/app/root/` 测试、`tests/pty_e2e_persistence.rs`。

现有入口只用于复用夹具，不证明当前已覆盖新契约。实现者按实际测试名记录可复现命令；至少 S-01 使用真实退出/新进程恢复，不能只用内存序列化往返替代。
