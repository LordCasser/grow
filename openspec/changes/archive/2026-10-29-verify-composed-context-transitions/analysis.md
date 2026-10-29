## 审计结论

确认两处可复现的状态衔接缺陷。

1. 最终普通请求准入计算 input + explicit output，但自动预压缩只判断 input 阈值。79K 输入、100K 窗口、25K 输出的组合直接在最终准入失败，未进入可用摘要路径。输出预算也没有沿 trigger 传入图片重投影、prune 和后台提升后的复核。
2. 冷恢复的活动 Goal 执行成功的 `/goal clear` 后，持久化 Control 和运行时授权均已清除，但 ChatState 仍沿用旧请求的 Goal scope。在新请求 build 前断言当前授权的 portable 压力，两种 route 顺序均失败（fixture 99 vs 101）。

## 修复

沿既有 AutoCompactTriggerInfo 传递实际显式输出上限，并复用现有自动压缩入口。普通输入阈值保持原语义；只有 input + output 真正超过总窗、且 output 本身小于窗口时，新增 reserve-only 恢复原因。None、0、精确总和边界、不可恢复的大输出预留不伪造额外预算。

prune 的输入目标取 threshold 和 window - output 中较小者；图片重投影复用相同触发判断。一次摘要或后台提升完成后先发送既有 Completed，再检查总需求；仍超窗结束本轮，不重复摘要或派发普通请求。抑制、memory flush 和 child task gate 保持既有约束。

Goal clear 在 durable Control 成功、运行时清除之后同步既有 acknowledged scope setter，再发布命令状态。持久化失败分支保留旧授权与 scope；同步失败返回错误，不回滚已落盘 clear。

## 证据边界

- 低值校准组合使用真实 ChatState actor 与受控 persistence；覆盖两次压缩、再增长、重复 build、inactive transaction 拒绝和 Surface identity。该拒绝发生在写入前，不冒称此组合完成了 ACK 丢失或断电验证。
- Goal 使用 validated Timeline restore 入口、SessionActor 实际 route admission、public rewind 和 GoalClear；不是完整 CLI 进程冷启动。任何 extant Goal 的 public rewind 都被原产品规则拒绝，必须先 clear。直接 ChatState rewind 仅作独立内部机制证据。
- 输出恢复使用脚本化 provider，核对实际请求数、Timeline 和通知。图片测试真实提交 ImageProjection 后因保留输入+输出仍超限进入 compaction transaction；其 fixture 没有可用 summary range，验证失败 gate 而非图片+成功摘要全流程。
- 真实文件与 SIGKILL 的已有恢复矩阵随压缩全量回归重跑；不把它扩大解释为真实 provider 或整机断电验证。

## 测试夹具纠正

首次新测试编译暴露 sibling helper 可见性、ConversationItem 不支持 PartialEq、mock receiver 被提前丢弃等夹具问题，已修正后执行行为验证。prune fixture 原先会被异步预阈值提前 prune，不能隔离 reserve-only 路径；改为先建立请求投影，再锚定74K/100K并预留27K（低于75%的异步预阈值）后才验证。摘要在 Surface 上的中性表示不固定为 Assistant，断言改为正文与确切 replacement 事实。
