## 不变量
- 冻结输入和同一后台 worker 在 promotion waiter 被丢弃时保持原 owner；重试等待不得重复创建 provider attempt 或重复记 Promoted。真正 cancel generation 后永不提交迟到摘要。
- 未确认的 durable ACK 必须向调用者暴露错误且阻断依赖执行。已确认 replacement 与未确认 terminal/repair 分开核对：不得撤销已提交替换、伪造未写入成功或在恢复时重复替换。
- pending async notice 描述已提交 replacement。只有后继普通请求完成投影后才能消费通知；仅换路由时使用新请求投影。若换模导致新的 replacement，则旧待发布通知失效，不用新 replacement 的计数标注旧事务。

## 方法
使用真实函数/SessionActor/Timeline 投影和可控 persistence ACK，区分写入前拒绝、实际事件已落盘但 ACK receiver 丢失，以及调用方 future drop。取消 provider 的测试必须释放阻塞结果并经过真实发布边界。记录所有失败探针及修复后的精确断言；不能用 sleep 扩时、跳过测试或重复计数规避。

## 范围
只处理这三个已授权组合及其直接根因。无关架构债务另列 backlog。

## 已确认根因与修复
两个真实 actor 探针均复现重复 Promoted 导致 CompactionNotOpen/fatal 错误。generation 本身并未重建，错误来自 durable 事实被按等待次数重复写入。

修复放在既有 RecordTimelineEventDurably 入口：仅对同一仍开放、已 Foreground 且尚无 Summary 的事务，返回 Timeline 中原 Promoted 事件。ChatState 的 live fold 只在 ACK 确认后应用事件，因此不会把尚未确认的磁盘可见数据当成功；排队的重试也必须等前一 commit 确认或 writer epoch 失败。关闭、换 id、已提交 summary 的事务不复用该事实。没有增加第二个 promoted bool、额外 registry 或新产品工具。
