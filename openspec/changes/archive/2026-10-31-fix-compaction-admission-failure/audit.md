# 压缩后 Cancelled 根因审计

本次是无现场报告的代码审计与受控复现，不能确认原截图的触发源。

## 内部准入路径

| 返回位置（turn/mod.rs） | 实际条件 | 压缩可否直接改变条件 |
| --- | --- | --- |
| 开头 terminal preemption | durable Behavior/Goal 转换已要求当前 Turn 结束 | 否；Goal 使用结算可间接触发 |
| next Step control boundary 缺失 | 第一步不再获准，或已有 Step 游标但没有可关闭 active Step | replacement 不清游标；正常 StepEnded/新 Turn/终态才清 |
| 压缩后的 Step admission | termination 非 Open、runtime_suspend、terminal_preemption、foreground 非当前 RegularTurn | 压缩不更换 foreground；这些条件来自独立控制/生命周期 |
| provider admission | 上述条件、缺失 active Step、storage gate 失败，被合并成 bool | **storage Err 经 is_ok 丢弃，是已确认的错误终态路径** |
| Stop-hook continuation | 终态检查后再次开始 Step 时已失去前台或已有 active Step | 压缩不直接设置这些状态 |

`AdmissionState::can_continue_regular_turn` 的生产写入来源：

- `termination`：shutdown、idle unload、control/worker/persistence fatal；只允许单调关闭。
- `runtime_suspend`：`request_node_suspension` 关闭，已提交恢复的 `release_runtime_restore` 才重新开放。`handle_prompt` 在最终结算前会 park，不能仅从内部 Cancelled 返回推断它一定发布到了客户端。
- `terminal_preemption_pending`：Behavior 转换，Goal 创建/重启/暂停/清除及 incomplete usage；精确 Turn 终态结算或真实取消清除。
- foreground：FIFO/Goal/runtime restore 建立 RegularTurn；settlement、Stop/rewind、teardown 移除。手动 compaction 只在 Idle 取得 Compaction owner，运行时只排队。
- active Step：`EventTracker::end_step`、`begin_turn`、`emit_turn_ended_transaction` 清除。压缩 replacement 和 Sideband 不调用这些游标清除入口。

另外，`process_conversation_turn_with_recovery` 在内部结果返回后重新执行 Goal spending 检查；因此 summary 期间的 Goal usage 停止未必最终呈现为 Cancelled，不能跳过该包装层来推断用户可见终态。

## 可复现缺陷

`storage_admission::StorageAdmission` 在另一 owner 报告 StorageFull/QuotaExceeded 后关闭共享域。当前 owner 若已完成摘要 replacement，可以仍成功确认 Compaction Completed；随后 provider gate 的 `check(...).is_ok()` 丢掉原始错误，并返回 `TurnOutcome::Cancelled { category: None, reason: provider admission closed before sampling }`。外层把它作为 Host cancellation 持久化。

修复只传播原 storage Err，保持原有控制优先级与禁止派发规则。单独归属另一 session 的 InvalidData 不应关闭本会话，这是负向对照。

## 异步取消的提交边界

- 未提升 worker 的 token 保存在 background slot。普通取消直接信号化 token；未完成 worker 留给原 owner 排空。
- publisher 取走 slot 后，`CompactCancelGate::enter_background` 保持同一个 token。取消不能只看 slot 是否存在；当前实现通过 gate 仍可触达 publisher。
- Summary ACK 前取消：后续 pre-replacement 检查拒绝提交；非提升后台的可恢复取消返回 false，保留普通前台。
- replacement 已提交、Completed ACK 尚未确认时取消：不能撤销已提交事实；Completed 保留。scope 退出后旧 token 不成为下一请求的取消权威。
- Control Reprojection 不重新取消当前摘要；`persist_control_snapshot_with_contexts_and_receipt_durably` 只有 Transition 会取消背景 generation，避免压缩自行重投影时取消自身。

上述两个 ACK 交错使用真实 ChatState 命令与普通 sampler HTTP mock 验证；ACK fixture 模拟存储确认，不模拟断电，不以 sleep 抢时序。

## 对抗性复核与范围

修改保持 `manual_compact → foreground → active Step → storage` 的原短路优先级；控制已结束时不会用存储错误覆盖它。`?` 返回会释放两个 admission guard，外层关闭 Step 并将普通 Err 结算为 error，不使用只适合未确认持久化边界的 fatal 错误包装。并发 Behavior 转换或 Goal spending 停止仍可按既有 recovery 规则接管旧回合结果。

另发现 Runtime suspend 的排空顺序值得独立验证：活动 compaction lease 分支等待 foreground stopped，而 foreground 在挂起 gate 关闭后 park 等待 owner 丢弃。此项尚未复现，也不解释正常运行中的 Cancelled，已登记 backlog，本次不改变挂起协议。
