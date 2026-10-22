## Context

SkillLoadSource::User 是 Runtime 已完成加载的事实；Model 来源已有真实工具往返。Responses adapter 当前把 User 事实展开为 function_call + function_call_output。thinking provider 会对这种模拟模型输出要求 reasoning_text，而事实没有这种内容。错误恢复只允许回放既有可见 reasoning，不能凭空修补。

## Decision

在 Responses adapter 中把 User 技能事实编码为独立 role=user 的 EasyInputMessage，正文使用既有 skill_load_result_content JSON，保留 source、input/reference/call identity、name、args 和完整 prepared_body。原始 User item 不拼接正文；Model 分支继续不额外输出。移除 Responses 此路径不再使用的 skill call-id map 参数，其他协议沿用现状。

继续复用 defer_agent_messages_after_tool_batch，把加载事实放在完整工具结果之后；native fragment 保持原文，用户技能作为增量上下文出现。预算估算仍沿用 ChatState 的 estimate_skill_load_tokens，按完整结构化结果加原调用包装计算；新 Responses 消息只减少包装，该估算保持保守，不会因本次变更漏计正文，不另建估算路径。

## Boundaries

不伪造空 thinking，不改变全局 reasoning 回放策略，不增加对 400 的盲目重试。真实历史工具调用仍受既有协议约束。这里只修用户技能投影；其他宿主事实的兼容性需独立证据后处理。

## Verification

先记录新回归在原实现失败，再运行 sampling-types 全量库测试、受影响编译检查和 OpenSpec strict/archive 验证。以原会话保存的请求结构作为复现依据；不向真实 provider 重发用户私密历史。
