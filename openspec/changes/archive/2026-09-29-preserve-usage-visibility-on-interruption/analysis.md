## Session evidence

本次只记录必要的计数与事件身份，不复制会话正文、请求体或凭据。样本为 `01a0eaf7-3acf-7872-8084-f4432d1c57a2` 的主 Timeline，截至 seq 1952；时间为 2026-09-29 UTC+8。原记录位于用户 Grow 会话存储中。

| 事件 | 时间 | 已确认事实 |
| --- | --- | --- |
| seq 129–137 | 10:22:44 | followup 从 FIFO 转入 steer；请求 `40c75cfb-95e5-4574-9eb7-4a7ad00d6310` 被取消，reason=`steered`；seq 136 的 attempt settlement 为 `usage: null` |
| seq 1375–1383 | 11:34:30 | followup 转入 steer；请求 `1bf3fddb-49eb-404d-84f0-39fef8810c27` 被取消，reason=`steered`；seq 1382 的 attempt settlement 为 `usage: null` |
| seq 1760 | 14:13:52 | 出现冷恢复边界，之前的 incomplete 事实继续属于 lifetime |

截至该边界的有效结算含 98 次主调用、2 次 Sideband 调用和 8 份子 Agent 汇总；主请求另有上述两次未知结算。累计已记录输入 36,049,160、输出 336,026、cache read 34,988,288，合计 36,385,186 token，与截图 `≥36.4M` 对应。所有已记录输入均有 cache read 数据，测量覆盖率为已记录输入的 100%，其命中率为 `34,988,288 / 36,049,160 = 97.0572%`。这不能证明两次未知请求的缓存或 token 消费。

## Contract and implementation

1. `openspec/specs/model-sampling/spec.md` 的 `Every provider attempt settles all applicable usage before readmission` 要求未知总消费保留 unknown，精确预算继续关闭准入。这一规则正确，打断请求不能按免费处理。
2. `chat-state/src/actor/mutations.rs::settle_model_attempt_usage` 对缺失 usage 调用 `session_usage.mark_incomplete()`；`actor/state.rs` 恢复同一事实。`usage.rs::UsageTotals` 已分别保存 `cached_read_tokens`、`cache_read_known_input_tokens` 和 `cache_read_unknown_calls`，无需重新设计账本。
3. `shell/src/session/actor/updates.rs` 将累计账本投影为 transient `grow/sessionUsage`。`pager/src/app/acp_handler/mod.rs` 替换累计快照并保留 incomplete，拒绝倒退/历史 replay；不是按每次通知重复累加。
4. `pager/src/views/agent_status.rs::session_usage_status_line` 在 `usage_is_incomplete || cache_read_unknown_calls > 0` 时直接显示 N/A。这遵循现行 `Ordinary agent status shows session usage`，但将完整账本与可用测量样本混为同一个展示条件。
5. `pager/src/app/status_blocks.rs::session_usage_block_text` 已能用已知 read 输入计算比例，并明确覆盖的是 recorded input。普通状态栏可复用这些计数与计算口径。

上述源码路径相对于 `crates/codegen/`。Atlas 用于有界符号定位；结论以当前源码、场景与测试正文复核为准。

## Cancellation accounting window

`sampler/src/stream/chat_completions.rs` 在 finish reason 后或独立 `choices=[]` usage 帧处接受完整 usage；随后仍可能等待流结束，现有可选尾部等待上限为 2 秒。`sampler/src/actor/request_task.rs::drive_l2` 取消会丢弃该 stream，`AttemptOutcome::Cancelled` 没有携带 usage，外层将其结算为 Incomplete。逻辑期限将 Cancelled 转为 `Failed { usage: None, ... }`，同样需要保留已确认账单。

这是源码可定位的窗口，尚未运行确定性复现；主会话的 `usage: null` 本身不足以证明 usage 帧曾到达。拟使用受控流按“finish → usage → 阻塞尾帧 → cancel”顺序复现，另以 usage 帧尚未到达的取消作对照。

必须保留现有完整性边界：Messages 的 terminal delta 只是累积状态，当前仅在 `message_stop`、stop reason 与 full input 构成均满足时形成完整 `TokenUsage`；Responses 使用原生 terminal snapshot。这两条路径确认完整 usage 后没有 Chat 式额外尾部等待，本项不扩大其 Known 判定。已排队的 L2 terminal 优先于同刻取消已有测试覆盖。

## Failure modes to keep distinct

| 条件 | 正确账本 | 正确展示 |
| --- | --- | --- |
| provider 已启动，打断时尚无完整 usage | 原 attempt 未知；已知总量为下界 | 有测量样本则显示 measured cache，否则 N/A |
| Chat 已确认完整 usage，结束帧仍 pending 时打断 | 原 attempt 的 Known 账单各记一次；请求仍为取消 | 已知缓存数据可用；总量是否完整仍由整个账本决定 |
| 总输入已知，部分请求 cache read 缺失 | 总量可精确；缓存分类部分未知 | 用已知样本分母显示 measured cache，详情给出覆盖率 |
| cache read 明确为 0 | 已知零命中 | 正常显示 0.00%，不当作缺失 |

现有自动测试的缺口集中在 Chat 已解码 usage 与取消的间隙，以及“已有样本 → 未知打断 → 后续样本”的状态栏联动；立项不将源码推断标为已通过运行验证。
