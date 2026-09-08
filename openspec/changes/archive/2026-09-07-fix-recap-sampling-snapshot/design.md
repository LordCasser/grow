## Evidence
- actor/turn/sampling.rs::reconstruct_full_config 对内部读取检查 model_route revision。
- prepare_chat_completion 构造 SamplingClient 后返回；client.rs 固定 base_url、backend/defaults。
- actor/recap.rs 随后再次 await get_sampling_config，且 request.model=Some。
- sampler/client.rs conversation_collect 仅在 request.model=None 时回退 defaults.model，因此无法纠正跨快照显式模型。

## Decision
复用准备完整 SamplingConfig 的路径，recap 在同一份配置上派生 client/model/context_window，不增加全局锁或限制用户切换模型。不为所有旁路调用顺带重构。

## Validation plan
补充受控配置切换测试，核对真实请求 endpoint/model 一致及预算取旧快照；保留已有 55 项 recap 回归。尚未有该交错的执行证据，不能标记修复完成。

## Implemented test seam
仅 cfg(test) thread-local 一次性 callback 在 client 准备后更改 ChatState 配置，两个本地 MockInferenceServer 检查实际 endpoint/model；不增加生产调度点，不使用随机 sleep。40KB 输入在原 256k 窗口完整保留，切换后的 8k 窗口不得影响本次预算。
