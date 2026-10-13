## 1. 输入身份与接纳核对

- [x] 1.1 在 prompt/queue/steer/followup 首次派发边界固定身份与请求快照，Shell 从原 prompt identity 关联 input_id，补齐影响执行的原始路由字段；测试精确重放返回原事实、不同内容冲突、新意图同文字可独立发送。
- [x] 1.2 串联 SubmitInputDurably、在途接纳 owner 与 Hook 结果复用；测试并发重发只运行一次接纳流程，Submitted 后 Hook 中断恢复为明确中断且不重跑副作用。
- [x] 1.3 实现 typed admission 状态核对及“明确未接纳”的证明条件；测试查询越过排队请求、ACK 丢失、完整 writer 替换、历史关联冲突/附件缺失，通信错误不得授权新身份自动重发。

## 2. 草稿与迟到响应

- [x] 2.1 将原提交身份与未确认 disposition 接入 InFlightPrompt 和既有单源 draft checkpoint；测试重启恢复、图片引用、草稿大小边界和 checkpoint 失败提示，不新增第二份草稿权威。
- [x] 2.2 接入原请求重试、已接纳合并、编辑后显式新发送及迟到 ACK 归属；测试新编辑内容不被清空、不同 session/binding 不串用、恢复队列不自动重复派发。

## 3. 恢复资源与清理资格

- [x] 3.1 从 Spawned/Seed/Result/Ended、input 和 notification 状态推导有限的恢复根；测试 surface_completion=false、Consumed/Dismissed、父终态缺失、正文缺失和损坏证据的保留/释放判定。
- [x] 3.2 将恢复根核对接入 session TTL 与 artifact 清理，使用已有 lease/publication gate 并在删除时复验；测试 scan 后新引用、活跃 writer、身份替换、拿锁失败及扫描超预算均不误删，正常已无依赖候选仍可清理。
- [x] 3.3 覆盖 worktree 正常收尾、创建失败回滚、普通 GC、独立 auto-GC 和 orphan snapshot 路径；验证无上层资格时保守跳过、含修改且 snapshot 失败时保留、未准入 pristine 例外仍可安全回滚，不引入 workspace→shell 依赖。
- [x] 3.4 给清理统计和观察快照补充资源归属/保留原因，固定扫描与内存预算；测试未扫描候选不被当作无引用、超时 worker 保留许可、保留项不算作已删除。

## 4. 存储故障准入与整体验证

- [x] 4.1 将确定的 owner/共享存储域写入故障接入既有准入控制；测试新 prompt promotion、child、provider/tool 被正确限制，而诊断/取消/修复可达且无关域不受误伤。
- [x] 4.2 实现合法 writer 持久屏障后的准入重评估；测试仅空间回升、旧 writer 未 drain 或 Goal/预算仍关闭时不得恢复执行，不用新业务调用探测存储。
- [x] 4.3 在隔离 fixture 联合验证“输入已提交 ACK 丢失→草稿恢复→同身份重试”和“子任务执行返回→提交失败→独立 GC→writer 恢复→结果交付”；断言无重复 Hook/provider/tool、用户修改保留、终态及用量不被改写。
- [x] 4.4 运行 chat-state/shell/pager/workspace/fast-worktree 受影响的定向 `cargo test --locked -p <crate> --lib <filter>`；在 verification.md 关联全部 Scenario 与实际结果，记录不可写加进程退出时的未持久内容限制，不进行真实磁盘填满实验。
- [x] 4.5 更新 `docs/development.md` 的输入未确认、清理保留与存储恢复说明；所有场景验证后执行 `openspec validate --all --strict --no-interactive`，归档并执行全量及 archive 校验。
