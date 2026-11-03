# 周边机制核对记录

本文件保留本轮只读调查证据，说明消息路径必须尊重哪些既有 owner 和边界。它不是重构范围，不定义其他上下文变更如何实现。用户已明确：其他层面的上下文变更不归消息投递方式管理。正式设计以 [消息投递分层](mental-model.md) 为准。

## 3. 现有机制的统一映射

下面“现有”表示已核对代码/主契约，不表示本轮重新执行了运行时测试。

| 机制 | 执行控制效果 | 上下文效果及实际边界 | 当前 owner / 证据入口 |
| --- | --- | --- | --- |
| 普通输入、followup、排队编辑（现有） | FIFO 仲裁；配置允许时已有输入可提升到 steer | HumanIntent 先 Hook/Allow，消费时发布输入；编辑替换保留 FIFO 位置/hold 规则 | `session/actor/input_admission.rs`、`prompt_queue.rs`；input-admission |
| 用户 steer（现有） | request 软抢占，等待其结算，再续同 Turn | exact TurnId；安全点消费；过期接纳拒绝，已接纳残留才 reroute FIFO | `session/actor/interjection.rs`、`session/actor/turn/sampling.rs:2152` |
| parent message / reply（现有） | interrupt=true 才请求抢占；reply 不授予上行抢占 | Received 先于信号；Consumed 生成 attributed input，idle wake 另受仲裁约束 | `session/actor/notification_drain.rs`；local-coordination |
| ask 回写（拟新增） | 不抢占、不主动唤醒 idle | canonical 成功来源派生 context-only 收据；安全边界消费；final-close 竞争按 cutoff | 本 change 的 local-coordination/session-timeline delta |
| Task/Monitor/Subagent 通知（现有） | 是否可开 Turn 由 source、owner、抑制状态与仲裁决定 | 通知不能跨 Goal/Plan owner；工具结果已表达的证据可 `Consumed{input:None}` | `notification_drain.rs`；session-timeline / behavior-goal |
| TaskStillRunning（现有） | 单独不 autostart | 下一真实 Turn 的上下文；说明 passive 机制已有基础 | `notification_autostarts`、`session/actor/turn/admission.rs` |
| model / effort / Agent 切换（现有） | 接纳 revision；不改当前 Step 正在执行的路由；idle 有 ApplyingControl owner | 完整 Step 后应用 cutoff 内控制；route/catalog/Agent context 按对应提交发布 | `session/actor/model_switch.rs`；behavior-state-overview / configuration-rules |
| Behavior picker（现有） | 按当前所有权判定可用性；当前 Turn 保留已捕获协议 | Control 可以先提交；现有边界检测到 Behavior 变化后结束旧 Turn，新协议在后继 Turn 生效；不把新协议塞进旧因果单元 | `session/behavior.rs`、`session/actor/session_mode.rs`；behavior-goal |
| Plan/Goal 控制工具（现有） | 提交成功后显式选择 Continue / ResampleStep / EndTurn；未开始的同批工具按控制结果结算 | Control 与 layer retirement/activation 同属已有协议；ResampleStep 不是新 Turn | `session/actor/turn/mod.rs:2802`、`goal_support.rs`；behavior-goal |
| Goal 生命周期（现有） | 控制未来续跑资格/预算；部分 host 命令组合终止原 exact foreground | Goal state、Control 与定义层退役；不能把 pause、普通 cancel 和关闭 session 当一回事 | `goal_support.rs`、`session/actor/turn/mod.rs`；behavior-goal |
| 异步/手动 compaction（现有） | 后台 generation 可独立生成；手动压缩有 Compaction foreground；迟到 worker 不能自行发布 | 校验冻结范围与 authority 后在既有边界替换；AutoContinue 只有获准后继 Step 才追加 | `session/actor/compaction.rs`、`idle_arbitration.rs`；context-compaction |
| rewind（现有） | 经过已有准入，可能另有文件回退事务；不是恢复旧计算栈 | 按 ConversationOnly/FilesOnly/All 选择回退对象；FilesOnly 不替换 Surface；不能恢复已失效权限/旧执行 owner | `session/actor/rewind.rs`；session-timeline |
| fork / resume_from（现有） | 创建新 owner，区别于原树执行恢复 | 继承选定历史/Seed，重绑来源；不继承原 pending 执行权或用户权限证据 | `session/fork.rs`；session-timeline / tool-authorization |
| Stop / Cancel（现有） | 对精确 owner 请求终结；request/tool→Step→Turn 结算 | 不等于删历史；队列和 pristine rewind 是明确的产品组合，不是 abort 的必然副作用 | `session/actor/tasks_cancel.rs`；input-admission / session-timeline |
| Suspend / Resume（现有） | 持久交接与原树恢复资格；不制造业务 Turn cancelled | 保存未消费输入、恢复位置及原所有权，按 checkpoint 恢复；不是 fork | `session/actor/suspension.rs`、`input_admission.rs`；session-timeline |
| Close / Quit / Delete（现有） | 按入口范围退役 runtime；停止新 admission，等待原 actor/persistence writer 所有权释放 | 关闭保留的历史与永久删除分开；删除检查真实关系，不能因 UI 行消失宣称完成 | `session/actor/teardown.rs`、`session/storage/jsonl/mod.rs`；client-surfaces / session-timeline |
| 权限/问题答复与 Sideband（现有） | 回复原 pending operation；是否停止由该操作的结构化结果决定 | 精确调用许可不等于新用户任务；Sideband 输出按 purpose 处理，分类/标题/摘要/咨询不能一律 inject | `session/pending_interaction.rs`、`session/actor/coordination.rs`；tool-authorization / local-coordination |

表内 Shell 路径以 `crates/codegen/shell/src/` 为根；某些行为跨多个实现点，主规范仍是已实现契约权威。
