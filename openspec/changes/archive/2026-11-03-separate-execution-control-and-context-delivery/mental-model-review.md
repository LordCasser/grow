# 消息投递分层的独立对抗性审查

审查日期：2026-11-03。独立审查者：`injection_design_review`。本次复核 `mental-model.md`、`surrounding-mechanisms.md`、proposal/design 的范围同步，以及与原 ask 回写设计的衔接，不修改运行时代码，也不替代后续实现验证。

范围已按用户最新两次澄清收窄：**分层对象仅为消息投递这一入口/方式内部的流程控制动作与上下文边界操作。** 此前把所有 Grow 控制/上下文变更纳入统一控制面的解释已被替代，不构成当前方案。周边机制表现在只保留为外部 owner/边界的只读核对证据。

## 结论

未发现阻塞当前限定范围设计交付的可行动问题。方案只在消息路径内部区分是否请求改变执行节奏，以及消息在哪个获准上下文边界消费；不存在统一上下文管理器、通用控制总线或其他上下文机制的重构清单。此前 [review.md](review.md) 的来源所有权和历史资格问题仍由新结构化成功证明解决。

## 已核对的关键边界

- **作用范围：** 独立 cancel 没有消息时明确在消息入口之外；只有“取消并发送”这类产品组合才涉及两者。request 软抢占与 Turn terminal 没有被合并，消息路径也不承担 Session/writer/树生命周期管理。
- **真正仲裁顺序：** 对照 `actor/idle_arbitration.rs::arbitrate_idle_wake`，先处理 pending Step control 和 manual compaction，再处理用户任务、通知和 Goal。心智模型没有用“有消息就运行”替代该顺序。
- **现有产品组合与新 inject：** Grow 用户 steer 保留“软抢占 request + exact Turn 后续消费”，parent interrupt 保留先收据后信号；DSH 非抢占 steer 与 Grow 不按名称合并。新增 ask inject 不进入作为抢占信号的 interjection buffer。
- **no-wake：** passive 收据不授予下一 Step 资格。§4 第 3 项明确 final-close 前仍可能延长活跃 Turn，但预算、取消和 Behavior gate 优先；无后继执行资格时保持 pending，不能强制 Goal 续跑或开启 Turn。
- **消费与模型可见性：** §4 第 5 项明确 Answered、Received、Consumed、provider 请求实际接收和模型认可分别是不同事实。Consumed 后、provider 请求前崩溃只恢复已提交 Surface，不重新消费，也不声称模型已读。
- **其他上下文变更不纳管：** §5 明确 model/Agent、Behavior/Goal/Plan Control、compaction、rewind、fork 由各自机制管理。消息路径只观察已发布的执行资格/owner/合法边界，不解释、封装或调度这些变更。fork 的原 pending 身份不被重复执行，是消息路径自己的消费约束，不是接管 fork。
- **故障与复杂度：** 终态证明与 Received/Consumed 没有变成两份竞争状态库；只读 replay 不修复，补齐仅属于合法 writer。首个实现增量仍是 ask 回写，不先改造全部消息入口或重写周边控制路径。

以上是源码/主契约与设计的一致性审查，不是运行时测试结果。未执行 Rust/DSH 测试、故障注入或新 release 构建。主 agent 负责将最终 delta/design 与心智模型同步并汇总 OpenSpec 严格校验；所有实现项继续保持未完成。
