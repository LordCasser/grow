## Context

当前数据流为 `compaction commit -> Surface delta projection -> AutoCompactCompleted -> next request build -> request projection`。Surface delta projection 有意保留最近 provider anchor 的请求 envelope，只用于在请求尚未重新物化时提供连续估算；它不是下一次模型请求的最终投影。Pager 同时用完成通知刷新上下文并永久渲染通知文本，而随后普通 meta 更新使用 request projection，因此两处会显示不同数值。

## Goals / Non-Goals

目标是让异步压缩通知和下一次普通请求使用同一个已物化 projection，并保持恰好一次通知。非目标是修改 token estimator、provider usage anchor、同步/手动压缩反馈、Pager 格式或持久化 schema。

## Decisions

1. **提交与展示分离。** 后台压缩成功提交时只保存 `tokens_before` 和从后台开始计的 `elapsed_ms`；完成通知不再读取提交后的 Surface-only projection。
2. **普通请求构建是最终值边界。** `build_request_for_image_mode` 成功返回意味着 ChatState 已对完整 request 执行 `apply_request_projection`。Shell 随后取出待发布记录，读取 `get_projected_tokens()` 并发布异步完成通知。
3. **单槽、恰好一次。** SessionActor 只允许一个后台压缩；待发布记录存在时也禁止启动下一轮后台压缩。发布前以 `take` 消费记录，后续重建请求不会重复通知。若该请求投影触发同步压缩，先发布异步通知再进入同步事务；若同步/手动压缩或 rewind 在下一请求前成功替换上下文，则新 replacement 取代旧展示语义并丢弃待发布记录。
4. **没有请求就不猜最终值。** 若压缩在已结束 turn 的边界提交，通知留待未来首个普通请求投影；session 在此前关闭则不产生完成通知。Timeline 中的压缩 Completed 仍是事务权威，UI 通知只是等待可解释数值的展示投影。
5. **同步路径不延迟。** promoted job 的调用方正在同步等待压缩，手动和同步自动压缩也有独立活动反馈；本次仅修复 `async_compact: true` 的语义错位。

## Risks / Trade-offs

- 已结束 turn 的异步压缩不会立刻出现 UI 完成行；这是刻意避免展示中间值，下一次真实请求形成后再显示。
- 如果下一请求组装失败，通知继续待发布；失败的 request 没有可称为最终值的 projection，不应消费它。
- 通知本身仍走既有 durable Grow update 路径；本次不新增可恢复的 pending-notice 存储格式。进程在下一请求前退出时，已提交压缩可从 Timeline 恢复，但不会补发旧 UI 通知。

## Validation

扩展真实 SessionActor + MockInferenceServer 异步场景：证明边界提交后没有完成通知；下一请求到达 provider 前通知已发布；`tokens_after` 等于此时 ChatState 的 request projection；后续重复边界不重复发布。保留 between-step、cross-turn、promoted、失败和取消回归，并运行 OpenSpec strict 校验。
