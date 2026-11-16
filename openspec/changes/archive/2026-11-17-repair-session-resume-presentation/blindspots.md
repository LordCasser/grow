# 实施前一轮盲区复查

2026-11-16，基线 `733d2c72e`。用户已授权本轮复查后实施。此记录只列会改变当前写法的发现，未核实的猜测不作为设计依据。

## 已确认并修订

1. **控制请求版本不等于当前状态版本。** 既有 `PublishControlState` 已带 actor epoch 和 admission 的请求 revision，但 pending/applying 仍可能没有应用。Behavior hydration 必须使用已确认的 Control revision 和同一 coordinator 状态，复用既有发布/查询链，不能把 pending target 当 current，也不能靠另建状态缓存解决。`actor/model_switch.rs::publish_control_state_snapshot`、`session_mode.rs` 的持久提交顺序是核对入口。
2. **历史 continuation 有三套字节坐标。** indexed response 是 encoded response 字节，retained text 是 UTF-8 字节，ToolOutputFragment 是序列化 Tool payload JSON 的字节区间。原 envelope 只写两类会遗漏 Tool。Before 从已交付区间 start 往旧读，After 从 endExclusive 往新读；token 必须签边界角色和坐标，不能把旧 before offset 换方向解释。
3. **有界 seek 的实现基础只在 indexed publication。** 现有无索引读页会重新读全 journal，再投影；不可据此声称所有路径都有有界磁盘读取。发布路径沿既有索引扩展；尚未具有索引的已验证来源在既有初始完整验证中捕获稳定的内存投影，随后页查询复用该固定投影而不逐次全扫。两种来源提供相同展示语义，性能验收分别报告。不能让普通已有会话的时间线跳转静默降为不可用。
4. **历史 cut 不能以 olderCursor 是否为空决定寿命。** 停在最旧页仍须持有 cut 以向新读/返回目标。cut 随 session/branch/view binding 退休；追加 live 不推进其 frontier。resident 的 cursor-tail reconnect 没有创建新 cut 时须保留原已安装 cut，不能用 None 覆盖而让仍打开的历史窗口失去续读能力。
5. **prompt_index 是位置，不能单独承担用量归属。** `chat-state actor/tests.rs::truncate_removes_items_after_target_prompt_index` 证明 rewind 后 next prompt index 可以复用。新增持久归属必须绑定既有稳定 TurnId/prompt_id；位置仅作一致性核对。last_turn_usage 是最近模型调用的上下文校准依据，不能按整轮消费累加恢复。

6. **Plan phase 的原地修改先于 gate。** Plan Submit/Amend、批准/拒绝和 no-client 路径会先修改 coordinator，再等待提交 helper；仅为查询加锁仍可能读到未确认 phase。实施将 candidate 构造、持久 ACK、live 采用与同版通知收敛到原 gate 内，失败保持旧状态。retained Plan review 同样必须先构造候选；完成/恢复等路径必须在释放 gate 前入队同版模式通知，避免通知参数与稍后捕获的状态属于不同选择。Control revision 也只能在 ACK 后发布，不能暴露预分配版本。
7. **child 的 owner 必须在委派准入时捕获。** 在最终 fold 时凭 attributable=true 查询当前 Turn 仍会误归属（客户端 prompt_id 可复用）。原 owner 随 pin/coordinator 传到最终账单；缺证据只保留 lifetime，不按当前 Turn 补造。`publish_turn_scope_resources` 在任务开始门释放前运行，此时新 Turn 尚未 durable，不能在那里读取新 owner；在 TurnStarted ACK 后、任何工具或采样前安装本 Turn owner，冷恢复同一 Turn 使用其原身份。Agent 重建必须保留这份只读资源。

8. **publication cut 与 load cut 不一定相同。** 批量索引只有未发布后缀达到 1 MiB/4096 records 或分支变化才推进；一个小的 InputConsumed 可以尚未进入已发布目录。磁盘目录测试必须明确它断言的是哪个 publication，实际 IndexedPages 目录则须合并同一 load cut 内已经验证的后缀 metadata，不能强制每次 append 发布或在每次目录查询重扫正文。后缀揭示旧 provisional source 时仍按真实 source 排序并保持 dense ordinal。

## 实施约束

- 不增加第二个 Behavior/Usage owner，不把当前投影与历史展示互相回写。
- 原非indexed支持与新indexed支持必须行为一致，但不伪造相同性能保证。
- 原有 TurnCompleted 足以承担结果留存；先复用已有查询/索引，只有实际调用方需要且现有入口不足才扩展，不新建结果日志。
- 依据修订后的共享契约按独立行为派发；实现返回新证据时只调整受影响边界，不再开启无界审计。
