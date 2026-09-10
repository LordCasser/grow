## Context

`process_conversation_turn` 在接纳合法响应后以 `tool_calls.is_empty()` 返回 Completed；`admission` 再写宿主 `end_turn`。该标签不是 provider 原始字段。已归档的历史投影修复保留真实调用事实，但不能使无工具行动预告获得后续执行。现有 Stop gate 是用户扩展，默认允许结束，不能承担内建完成协议。

## Decisions

1. 使用已有工具协议承载完成意图，而不是从文本语义、冒号、Responses phase 或 provider stop reason 猜测。`FinishTurn` 由 shell 提供并拦截，和 StructuredOutput 一样不进入业务工具派发。它只声明本 Turn 的完成/等待状态，不修改 Goal、权限或业务状态。
2. 普通 Turn 每次采样均提供该控制工具；工具描述说明必须先向用户输出最终答案或明确问题，然后单独调用 FinishTurn。业务工具调用表示继续工作。`--json-schema` 使用既有结构化输出协议，不同时要求 FinishTurn。
3. 先持久化原始响应。FinishTurn 必须是唯一调用、参数严格有效且该响应有用户可见文本。混合批次拒绝完成声明，真实业务工具按原权限链执行。多个/无效声明追加错误结果，不能到业务派发。缺失声明追加 synthetic auto-recovery 上下文并继续同一 Turn。连续三次违约后返回明确错误；有实际业务调用或明确完成后不继承前一次连续失败。计数不因模型切换重置。
4. 不重放已执行工具，不重写旧 assistant 内容、不合成 provider stop reason。FinishTurn 的调用与结果是可恢复证据，Turn terminal 的 completion_kind 区分显式完成与等待。最终 Stop hook 仍可阻止结束；它触发新 Step 时必须重新取得完成声明。
5. 复用正常循环的 StepEnded、控制准入、Goal 预算及取消检查。终止权优先级保持：取消/预算/控制/拒绝优先于协议恢复；结构化输出使用已有校验。来自旧 Step 的完成声明不能授权后来的用户输入，排空待处理输入后须重新采样。

## Alternatives

- 标点或行动预告词表：语义不可靠，跨语言及正常回答均会误伤。
- 额外 LLM 分类器：增加一套判定、预算和失败机制，仍然把完成决策隐藏在启发式里。
- 只保留 Responses phase：不能覆盖没有该字段的 Messages/Chat；phase 的完整持久化另行登记。
- 强制只通过 JSON 返回所有回答：破坏普通流式文本体验及现有工具循环。

## Risks and Validation

完成声明是模型的显式主张，不是对业务结果正确性的形式化证明；模型仍可能错误声明完成。协议能保证普通文本响应不会被宿主默认为完成，并能定位明确声明。回归覆盖三种原始协议终止后的预告→工具→完成、正常回答/等待、混合调用、错误参数、连续失败、取消/预算、既有截断恢复与 Stop gate。监测磁盘，仅复用当前 target 做相关 crate 测试，不制作 release 包。
