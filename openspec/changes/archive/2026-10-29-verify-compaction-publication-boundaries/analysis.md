# 压缩发布组合边界复核

## 发现并修复：重复等待导致重复提升

原压缩 worker 已完成 durable Promoted，但持有它的等待 future 被丢弃。BackgroundCompactionOwner 将原 worker 还回 slot；下一次等待再次写 Promoted，Timeline 返回 CompactionNotOpen，Shell 将它升级为致命持久化边界错误。修复前两个真实 SessionActor 探针均失败，输出保存在 `probe-before.txt`。

修复仅在 ChatState 的 durable 事件入口复用已确认的同一 Promoted：generation 必须仍开放、处于 Foreground、尚未记录 Summary。原 Timeline 的严格状态迁移不变；已关闭、不同 ID 或已提交摘要不能借用旧事实。原命令必须先完成 ACK 才会进入 live fold，排队重试无法提前成功。没有增加第二份 promotion 状态、计费记录或重试任务。

## ACK 注入边界

测试用 TimelinePersistence sink 在内存保留不可变事件后，分别丢弃、挂起或拒绝 ACK；它模拟“事件可恢复但调用者不知道成功”，不等同于真实磁盘断电测试。

- replacement / Completed 首次 ACK 丢失：原写入循环精确重试同一 seq 与 payload，确认后只应用一次。
- replacement ACK 始终挂起后取消 actor：调用失败，后继写入被拒绝；保留日志冷投影恢复 replacement 并闭合一次 Completed。
- Completed ACK 挂起到既有 deadline：调用失败并关闭 writer；冷投影保留原 Completed，不重复闭合。
- Promoted ACK 未确认时丢弃原 waiter，直接 poll 第二次提交：必须保持 Pending；原 ACK 成功后返回同一 seq，失败时排队提交也失败。
- replacement 已确认，必要 AgentRole 重投影事件已保留但 ACK 丢失，精确重试再遇永久错误：检查真实压缩调用失败、后继 ChatState 写入被拒绝、冷恢复保留摘要和控制上下文，并闭合已提交 replacement。

## 路由与异步完成通知

两条用例均从真实后台摘要请求开始，经完成 worker 和 publication boundary 提交 replacement；不是手工设置 pending notice。

- 同窗口切换经正式模型选择入口更新 route 和 ChatState；通知在切换时保持待发布。后继普通请求经真实 turn loop 组装后，恰好一次通知采用当前投影，provider 请求绑定新模型。
- 降窗前追加合法历史，使投影超过新 80k 窗口。模型选择后触发真实同步压缩；新 replacement 作废旧异步通知，只发布新同步完成，保留尾部后应落在新窗口内。

初版测试中，同窗口请求已正常结束，但 mock 按 headers/tool 数的粗分类未匹配 foreground expectation；另一个夹具以普通 user_meta 伪装 AgentRole，被 Control 校验拒绝。两者均按真实契约修正夹具，不放宽产品校验。

## 对抗性复查

核对了 actor 单消费者命令序列、query waiter drop 的行为、commit 在 ACK 后才 apply 的顺序、poison 后 mailbox 关闭、generation ID 不复用和 Summary/terminal 后的严格拒绝。所有取消路径继续使用原 token；真正取消的 generation 即使再次提升并收到迟到输出，也不能提交。验证结果和范围限制见 `verification.md`。
