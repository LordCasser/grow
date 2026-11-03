# 本地实现对照

调查日期：2026-11-03。Grow 工作区已有大量未提交的前序改动；本轮只新增本 change 文档。Atlas scoped 查询存在旧位置，结论均回到当前源码核对，未把窄搜索空结果视为全库不存在。

## DSH

用户指定 checkout：`/Users/lordcasser/workspace/projects/deepseek-harness`，HEAD `ce7073d51254c26cd63b22a60835b8f948deb447`。它是另一个带未提交内容的工作区；本轮仅阅读，未修改、安装或执行其测试。下列为当前本地文件证据，不宣称已实测上游运行时。

| 证据 | 可确认的机制 |
| --- | --- |
| `packages/core/agent-loop/src/agent.ts:170–196` | send 独立选择 next-step/next-turn 与 wakeup；followup、steer、inject 分别组合它们。cancel 单独 abort，默认清 inbox，keepInbox 可保留。 |
| `agent.ts:170–177` | wakingAfterAbort 只对带 wakeup 的发送改路由；不能推导被动 inject 在 abort 期间自动变成 next-turn 或自动唤醒。 |
| `packages/core/agent-loop/src/inbox.ts:109–113` | claim 总是先取 next-step；新 Turn 再取一个 next-turn。因此 idle inject 可在随后真实 Turn 的第一个 Step 进入。 |
| `packages/core/agent-loop/tests/agent.spec.ts:42` | idle inject 测试断言只留下 inbox splice、状态 idle、零 provider 请求。 |
| `packages/core/agent-loop/tests/resume.spec.ts:1014` | 测试覆盖 pending idle inject 的持久恢复；fixture 模拟非正常前生命周期，并明确 graceful dispose 会清 pending，不能据此宣称任意关闭都保留。该测试跳过 Windows。 |
| `packages/core/agent-loop/tests/cancel.spec.ts` | 含 keepInbox、abort-to-idle 期间醒来消息、清 inbox 与 wake latch 的测试。 |

调查起初也查阅了用户给出的 [上游 agent.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/agent.ts) 与 [inbox.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/inbox.ts)。远端 master 的文件行号/版本与本地不同；最终设计依据上述本地 checkout，不把远端缓存页视为本地快照。

## Grow

| 当前源码/契约 | 可确认的机制与边界 |
| --- | --- |
| `shell/src/session/actor/interjection.rs::admit_human_steer` | 在 input admission 与 step control 栅栏内捕获 exact TurnId；Hook 后再次校验。显式旧目标返回 StaleSteerTarget，不自动交给新 Turn。 |
| `interjection.rs::discard_residual_interjections_at_turn_end` | 已 Allow 的残留 steer 以原 input identity durable reroute 到 FIFO；与接纳前即已过期的 steer 不同。 |
| `shell/src/session/actor/turn/sampling.rs:2152–2213` | pending user interjection 或 parent_message_interrupt 会取消当前 sampler request，等待 collection/stream 结算，再在同一 Turn 续采样。与 DSH steer 的非抢占语义不同。 |
| `interjection.rs::close_steering_and_drain` | 最终回复与新输入存在关闭栅栏；当前只对 parent/reply 类型检查通知。新增被动来源必须明确最终 Step 竞争，不能只增加 enum。 |
| `shell/src/session/actor/notification_drain.rs::receive_agent_message` | 消息先 durable Received，再对 interrupt=true 的仍 pending 收据发抢占信号。AgentReply 不允许上行 interrupt。 |
| `turn/sampling.rs:2157`、`actor/planning.rs:1069` | parent interrupt 分别被模型请求等待和依赖等待观察；“只在 planning 被观察”的初步窄搜索结论已排除。 |
| `notification_drain.rs::notification_autostarts` | 当前仅 TaskStillRunning 不自动开启 Turn；不能把新的 inquiry 收据当普通完成通知直接塞入，否则会错误唤醒。 |
| `notification_drain.rs::drain_active_notifications`、`turn/admission.rs` | 活跃 Turn 在安全点消费；首个采样前也有通知消费入口。必须覆盖这些实际请求路径，不只测试 helper。 |
| `chat-state/src/timeline.rs::NotificationEvent` | Received 保存来源和 immutable payload reference；Consumed 在同一事实中提交消费与精确模型输入。无须另建消费状态库。 |
| `shell/src/session/notification_inbox.rs` | artifact 只拥有正文，Timeline 拥有排序、接收与消费。 |
| `actor/coordination.rs::run_coordination_inquiry_queue` | Sideband 回答先完成自己的结果，再写目标 terminal audit，最后答复调用者；已有完成事实可复用，不能让回写重新调用 Sideband。 |
| `actor/coordination.rs::existing_coordination_outcome` | 相同 identity/outcome 重试直接复用，新增同步义务必须同时覆盖此分支及冷恢复。 |
| `openspec/specs/local-coordination/spec.md` | Parent-child、tool evidence、context visibility、opinion reply 均明确禁止目标 Sideband 自动写回；本 change 必须同时修改，不能只改一处。 |
| `docs/architecture/input-routing.md` | 真实 HumanIntent 的 Hook/Allow、FIFO/Steer 和权限身份不可借给合成通知。idle 仲裁保留 user FIFO 优先及 Goal/Plan 所有权。 |

以上 crate 路径位于 `crates/codegen/`。当前源码说明 Grow 已有分层基础，同时部分产品动作把抢占和投递组合在一起；没有证据需要重写 loop。测试列表只是已阅读的覆盖，不代表本轮执行过测试。
