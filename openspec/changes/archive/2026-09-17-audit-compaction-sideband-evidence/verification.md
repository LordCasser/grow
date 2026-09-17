# 核对结果

- 默认配置为 `verbatim_input=true`：`shell/src/agent/config.rs::resolve_compaction_verbatim_input`。`prepare_compaction` 从冻结 Surface 选择待替换历史范围，将其交给 `prepare_conversation_for_verbatim_summarization`；完整工具尾部保留，没有 `/btw` 原来的 ToolResult/Assistant 无条件裁尾循环。
- `chat-state/src/compaction_utils.rs::truncate_trailing_incomplete_tool_call` 只处理最后是 Assistant(tool_calls) 的情况；已有 `verbatim_keeps_trailing_complete_tool_run` 和 `verbatim_keeps_tool_calls_args_and_results` 直接验证完整工具结果保留。不能据此声称此 helper 对任意畸形历史都有完整配对验证。
- 只有明确 `context_overflow` 才推进 `Verbatim → VerbatimFitted → Simplified`，普通 transient 重试不触发该阶梯（`shell/src/session/actor/compaction.rs:1297`）。Fitted 会按预算移除较旧输入或截断保留单元；它不是无损请求重试。
- 此处 overflow 包含 provider 上下文超长和本地 Sideband 预算拒绝：`summary_compaction.rs::sideband_error_to_sample_error` 将 `AttemptBudgetExceeded` 映射为可驱动降级阶梯的确定性错误；已有同文件测试验证该分类，本次仅核对源码。
- `verbatim_input=false` 会直接进入 Simplified。该路径和最后一级 overflow 降级都调用 `prepare_conversation_for_summarization`；其 `strip_tool_messages_for_conversation_item` 对所有 ToolResult 返回 None，并把调用降为仅含工具名的 `[Called tools: ...]`，因此结果正文、结果附件、调用参数均不进入该摘要输入。
- `ShellCompactionSampler::sample_compaction` 与 `build_compaction_request_surface` 只在收到的选区输入后追加摘要指令，provider 发送不会补回被过滤的结果。
- `commit_generated_compaction` 继续使用最初 `range_plan.target` 替换主 Surface，没有根据 Fitted/Simplified 实际保留输入缩小 target。未选中的较新 tail 保留，原始 Timeline 事实也仍存在；风险是摘要及后续主模型活动上下文缺少旧执行证据，不是持久账本被物理删除。
- 现有 `context-compaction` 主规范明确保护范围外 tail 和后续结构化工具往返，尚未要求摘要降级保留选区内部的全部执行证据。这是需独立设计的输入保真债务，不能用 tail 保留测试代替摘要输入验证。

## 验证

`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib compaction_utils::tests:: -- --test-threads=4`：132 passed，0 failed。包含完整尾部保留、原文调用/参数/结果保留、simplified 删除工具结果、预算裁剪及选区边界测试。

源码核对覆盖默认配置与生产降级/提交接线；未执行 provider overflow 阶梯的端到端故障注入，也未读取用户原会话账本。

纯审计 change 已用 `--skip-specs` 归档，未修改主规范。全量严格校验归档前 18/18、归档后 17/17 通过；归档完成度校验 339/339 通过，`git diff --check` 通过。
