## 验证路径
子进程使用真实 persistence channel 和 JSONL writer。测试 wrapper 仅在生产 writer 已确认 file/directory durability 后截住返回 ChatState 的 ACK，写出切点证据，再保持原 ACK 未确认。父进程仅终止其创建的子进程，并 wait 回收；进程退出前另一个 writer 必须受 OS lease 排斥。

恢复进程走 persistence::load_light(claim_writer=true)、ChatStateActor::spawn_from_validated_timeline、recover_interrupted_durably 和 SessionActor 的 Control 修复。再次启动独立恢复进程，比较不可变 prefix 和事实数量，验证恢复自身也可被中断且幂等。不把 Timeline::from_events 的内存模拟当作重启结果。

## 不变量
- 未 replacement 的原历史不丢，已 replacement 的摘要和未选中 Surface IDs 保留；摘要 Sideband 来源/结果从实际文件验证。
- 每个事务最多一个 replacement 与 terminal；已落盘但 ACK 丢失的 Completed 不重复追加。
- active Control context 在后继采样前恢复一次，已重投影的上下文不复制；缺少必要 artifact 或完整记录被篡改时拒绝写者恢复。
- ACK 未确认时原调用不能成功；崩溃不恢复原后台 worker，也不自动重跑 provider。

## 范围限制
这是真实文件和进程死亡验证，不模拟整机断电或存储硬件丢失已确认 sync。摘要文本是确定性 fixture，以精准控制持久化边界；不依赖线上模型。

## 已确认缺陷与修复边界
真实子进程测试在 Summary 已落盘而 replacement 尚未提交的分支发现重复恢复失败：首次恢复合法追加 Failed，后继 writer load 的 cross-ledger 校验仍只允许 open Summary 无 replacement，拒绝已合法终结的 Failed。原测试只对 open Timeline 执行内存恢复，未将终态写入后再次启动，因而未覆盖该反例。

同一校验还按 SurfaceRange 搜索整条历史，可能把失败事务之后相同选区重试的 replacement 关联给旧 Summary。修复只在该跨 ledger 校验入口限定当前 Summary 到其同 ID terminal 的事务范围，允许已由严格 Timeline lifecycle 确认的 Failed 无 replacement。Sideband 完成状态、原始字符数、目标与 canonical prefix 校验保留，Completed 缺少 replacement 仍由 Timeline 拒绝，不新增索引或放宽完整记录校验。
