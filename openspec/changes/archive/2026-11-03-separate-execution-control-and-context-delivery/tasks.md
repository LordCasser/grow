## 1. Architecture analysis — this delivery

- [x] 1.1 对照用户指定的本地 DSH send/inbox/cancel/resume 源码和测试，记录实际版本及未执行验证。
- [x] 1.2 核对 Grow 当前源码与 input/coordination/Timeline 主契约；区分用户 steer 的 request 软抢占与 DSH 非抢占 steer。
- [x] 1.3 固定职责、首个问答回写闭环、no-wake、最终 Step、取消/恢复、权限及故障语义，建立 delta。
- [x] 1.4 完成独立对抗性设计审查，修正问题并验证本 change 与全量 OpenSpec；本轮不把下面的实现项勾成完成。
- [x] 1.5 完成消息投递路径分层的对抗性复审：核对流程控制与上下文消费、各自 owner/边界；其他上下文变更仅为外部约束，修订后继执行资格与消费后尚未发送请求的场景。

## 1a. Runnable state-machine demo and plan revision

- [x] 1a.1 建立 stdlib 转移模型、真实动作轨迹重放、有界状态探索与行为变异反例；明确它不证明 Rust 运行时。
- [x] 1a.2 修正模型漏检：dispatch 必须满足冻结 terminal 义务而不只 Received，消费/ACK/发送分开，动态预算/取消交错；主 agent 独立运行最终 demo 通过。
- [x] 1a.3 按反例修订候选快照/最终关闭、错误传播、成功引用和后缀协调规划，写入 implementation-task.md 后派发首个端到端实现。

## 2. First implementation increment — implemented and verified

- [x] 2.1 给新成功 inquiry terminal 绑定原始回答方 target identity 与实际 Sideband/owner 证明，接入唯一的 context-only Notification 来源/稳定身份/正文，不增加新的 loop、消息 store 或人类输入，也不由旧 Answered 或 fork 继承事实制造同步义务。
- [x] 2.2 在现有通知 admission/active drain/first sampling/final-close 边界落实非抢占、no-wake；保留现有 steer 产品组合、FIFO/编辑 hold 与 Goal/Plan/Workflow 所有权。
- [x] 2.3 在 live reconciliation、已有 outcome 重试与 replacement writer 恢复中补齐 terminal→Received 的缺口；不二次采样、不重复消费、不把 Answered 改写成失败。
- [x] 2.4 更新开发者说明及必要持久 schema；不提前把目标行为当作主规范事实。

## 3. Tier 3 verification and closure

- [x] 3.1 真实 actor/provider fixture：child 问 parent、parent 问 child、peer 问 session；提问方只有原工具结果，回答方下一请求含完整归属问答且 PermissionEvidence 为空。
- [x] 3.2 控制住在途 stream 和工具：被动接纳不触发 sampler cancel/工具取消；工具结果先结算，下一 Step 才见注入。保留既有用户 steer 的同 Turn 软抢占回归。
- [x] 3.3 final-close 前后两种确定性交错：前者续同 Turn 的 Step，后者 pending 到下一真实 Turn；同 Agent 没有两个 foreground、结束 Turn 没有新输入。
- [x] 3.4 idle 单独 inject 零新增请求/Turn；下次用户输入、已允许的 Goal/Workflow/notification 首个请求可见；held FIFO、暂停/停止 Goal 及 Ctrl+C suppression 不被绕过。
- [x] 3.5 cancel-to-idle、挂起恢复：pending 保留且不主动唤醒；显式旧 Turn steer 仍拒绝，已允许 residual steer 保留原 identity/FIFO 回退。
- [x] 3.6 注入成功 terminal 前后、Received 前后、Consumed ACK 前后故障；同 inquiry 重试/冲突、live 核对、冷恢复、同名不同 source、重复 UI replay；无新证明的历史 Answered 不回填。既有成功回答不回滚，不重跑 Sideband。
- [x] 3.7 三类 ask 取消/拒绝/失败不产生成功上下文；外部 API 不向来源合成输入；其他 Sideband 不回写；角色/权限分类不能从问答取得人类许可。
- [x] 3.8 fork/compaction/portable projection 与冷恢复保留归属、真实正文和坐标；不转移 pending 消费权；特别验证 fork 只有源 Session 的成功证明、没有本地收据时也不会重建出待办。独立审查最终实际 diff 与验证记录，运行适用测试和 OpenSpec 严格校验后再归档。
- [x] 3.9 final-close 前已有被动收据，但取消/预算/控制变化先关闭后继执行资格：不能强开 Step；Consumed 已提交、provider 尚未接到请求的恢复不重复消费或声称模型已读取。

## 4. Joint-journey completion, independent review, archive and release

- [x] 4.1 按用户授权补齐工具/子任务等待、双向真实路由、Goal 暂停/预算恢复、删除排队/运行中询问及剩余关键联合旅程；逐项保存真实断言与运行证据，发现问题先更新相关 delta 再修复。
- [x] 4.2 独立审查最终新增代码、联合测试及失败路径；修正后复核，核对第 3 节每项场景的实际证据，完成相关库回归及静态检查。
- [x] 4.3 所有必要验证通过后归档本 change，检查主规范无旧隔离语义残留，全量和 archived 严格校验通过，修复归档导致的文档链接。
- [x] 4.4 归档后的最终源码编译本地默认特性 release，验证版本、启动、实际双进程协调回归、文件类型/签名与源码指纹，记录产物路径与 SHA256；在构建活动结束后清理可回收开发产物。
