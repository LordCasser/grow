# 对抗性审查

范围：本轮 active 列表、单一 subagent 结果读取、CommandsSettled 的 Tasks 恢复，以及用户补充的多目标等待。下面四项是独立债务，本 change 只实施 wait-any。

## A1：默认枚举仍克隆完整来源历史（P2，既有路径残留）

`ChannelBackend::list_subagents` 即便 scope=active 仍并行读取 source history；`observation::read_source` 对 resident source 调用 `timeline_events()`。ChatState actor 的 GetTimelineEvents 同步对全部事件 `to_vec()`，包含不相关历史及 owned payload。调用成本仍随完整 Timeline 增长，处理该消息期间不能处理其他 actor 命令，外层 timeout 不能抢占同步 clone。active 已减少历史 child 的读取，但未消除来源历史复制；未运行 1000M token 负载，不声称它已造成某次现场超时。

边界：在 owner 侧提供必要的生命周期/授权投影，仍保留 Spawn/Seed/Ended/Result 的核验，不简单绕过历史权限。验证应固定活跃数量、递增无关历史，同时观测 actor 控制消息延迟。

证据：`shell/src/agent/subagent/observation.rs:83`；`chat-state/src/actor/mod.rs:1097`；`tools/src/implementations/grow_build/task/backend.rs:606`。

## A2：运行时沉默会耗尽历史回退预算（P2，既有路径被新入口复用）

`ChannelBackend::query` 等 coordinator 直到同一个 deadline；收到 TimedOut 后再调用 `read_history_inspection`，后者发现 deadline 已到就退出，history callback 没有机会执行。独立探针链接现有 Tools 生产库，以相同历史 callback 对比两种 transport 状态：保留但不回复 channel 时 `Err(TimedOut), history_reads=0`；关闭 channel 时立即得到 `Completed(output=verified answer), history_reads=1`。探针 exit=0，临时目录已清理。证明的是生产 backend 调度，不是磁盘恢复 E2E。

边界：在原总预算内安排 canonical 读取，使其不依赖运行时必须先回复；不增加串行超时或重试墙。回归应让 coordinator 静默且历史可确认，断言仍能返回可信终态。

证据：`tools/src/implementations/grow_build/task/backend.rs:881`、`:391`；`subagent/mod.rs:132`。

## A3：主动取得答案仍保留同一完成通知（P2，设计缺口）

`consumed_completion_ids` 对 `SubagentResult` 返回空数组，和上次“读取不消费通知”的设计一致。独立探针对含 Completed 和完整答案的生产 ToolOutput 实测 consumed IDs=[]。若 child 的完成通知尚未交付，get_subagent_result 先交付答案，随后通知 drain 仍会交付同一结论，可能引发重复分析或额外模型轮次。结果可重复读取与通知是否已交付是不同责任。

边界：单独修改契约，考虑把实际交付的终态结果计入 receipt acknowledgement，同时保留 durable 结果重复读取能力。需覆盖主动查询先于通知 drain 的时序、再次读取和输出不可用，不能只用返回固定 snapshot 的 stub 证明通知一致性。当前仅完成代码链路审查和消费 ID 探针，未跑实际 SessionActor 重复投递 E2E。

证据：`tools/src/reminders/task_completion.rs:322`；`shell/src/session/actor/tool/result.rs:331,506`；`notification_drain.rs`。

## A4：恢复耗时从转后台时间算起（P2）

恢复投影把 TaskBackgrounded 的 historyTimestampMs 当进程 start_time，把 CommandsSettled 落盘时间当 end_time。命令先前台运行 60 秒、转后台 10 秒后挂起，冷恢复得到约 10 秒而非约 70 秒。真实 command snapshot 在挂起结算处已经存在，但 RuntimeCommandSettlement 没有保存其起止时间。现有 PTY 直接启动后台任务且未断言耗时，无法覆盖前台转后台路径。

边界：单独保存/投影实际执行时间并覆盖前台转后台再挂起恢复；本轮 Tasks 终态恢复仍有效，但不能据此宣称精确保留原耗时。本项为代码时序分析，尚未执行该 PTY 反例。

证据：`shell/src/session/history/projector.rs:489,512`；`shell/src/session/actor/suspension.rs:975`；`tools/src/implementations/grow_build/bash/mod.rs:2142`。

## 本次收口：多目标等待的 wait-all 阻塞

用户明确要求 any 子任务终态即唤醒并分析结果。旧代码 join_all 与旧动态提示词都要求 wait-all，属于已确认需要替换的行为。本 change 的 proposal/design/delta 和验证记录覆盖该变更，其他四项不混入生产实现。
