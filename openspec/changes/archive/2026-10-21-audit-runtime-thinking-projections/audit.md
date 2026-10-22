# Runtime thinking 投影审计

范围：在 Responses 用户技能投影修复之后，检查三协议的 Runtime 事实、reasoning 恢复和工具配对。本次仅分析，不实施后续修复。

## 结论

上轮修复仍有效，但只覆盖 Responses + 用户主动加载技能。还存在两处同类 thinking 字段遗漏，以及一处生产技能发布与历史修复不一致的问题。另有 native 分段插话边界的 adapter 反例，其生产可达性需要单独验证。

## A. Responses 跨 Agent 消息仍生成无 reasoning 的模拟调用（P2）

`sampling-types/src/conversation.rs:4258` 将每个 AgentMessage 编成 `receive_agent_message` function_call/output。没有关联模型 reasoning；portable 已学习模式也不会增加 reasoning。事实本来是宿主交付的消息，和上轮 User SkillLoad 的触发机制相同。

生产入口：`shell/src/session/actor/notification_drain.rs:315` 构造 AgentMessageItem 并通过 Notification::Consumed 发布；`turn/mod.rs:1680` 在下一采样前消费活跃通知。收到父 Agent 消息或回复即可走到此路径，无需用户加载技能。

离线探针确认未学习与已学习 Responses 路由都得到 user → function_call → function_call_output，零 reasoning items。对于和原会话一样要求工具调用关联 reasoning_text 的路由，仍存在同类 400 风险；本次未向真实 provider 发请求，不宣称所有 Responses provider 必然拒绝。

修复方向：让宿主消息保留来源/回复身份，以非模型输出表达；应独立确定消息角色与信任边界，不冒充用户授权或模型思考，也不能简单重试相同 body。

## B. Chat 技能/跨 Agent 消息绕过 reasoning_content 恢复（P2）

`conversation.rs:3550` 与 `3567` 直接构造 assistant_tool_call 后 continue；只在普通 assistant 分支 `3585` 才补 reasoning_content。探针开启已学习 Chat 模式，同一请求的普通 assistant 正确得到空字符串，但 Runtime 生成的 assistant 完全没有该字段。这与当前 model-sampling 中“Chat 无可见 reasoning 时编码空字符串”的契约存在缺口。

恢复门也漏了这类 wire assistant：`chat-state/src/actor/mod.rs:856` 只看 portable ConversationItem::Assistant 与 native Chat fragment。只有 User + SkillLoad/AgentMessage 且无 native 的历史，转换前没有 Assistant，无法确认启用；即使混有普通 Assistant 而启用成功，A/B 的 synthetic 分支仍遗漏。

因此切换到要求 reasoning_content 的 Chat 路由后，用户技能加载和跨 Agent 消息仍可能触发拒绝，不能靠现有一次性恢复消除。没有真实 provider 在线验收；确定的是字段遗漏和恢复条件不覆盖。

修复方向：投影和“是否改变 wire”的恢复判定必须覆盖同一集合；若保留模拟 assistant 形式，应按既有契约编码缺省字段，不能伪造正文。

## C. 模型技能事实打断工具结果，历史修复丢弃真实结果（P1）

可达源结构：模型同批次发出 load_skill(a) 和另一个工具(b)，技能结果先完成。

```
Assistant calls [a, b]
ToolResult a
SkillLoad(source=Model, call_id=a)
ToolResult b (真实成功)
```

生产链：`shell/.../tool/mod.rs:558` 逐个消费完成结果，技能专用分支执行 `finish_model_skill_load` 后继续处理兄弟；`skill_loading.rs:87` 调用条件发布；`chat-state/src/actor/mutations.rs:407` 原子追加 `[ToolResult, SkillLoad]`。load_skill 没有隔离整个工具批次的声明，该形状不是人为虚构的不合法重复 ID。

`request_builder.rs:35` 请求前执行 `ensure_conversation_integrity_durably(UserCancelled)`。`compaction_utils.rs:1175` 的 strip_displaced_tool_results 在 SkillLoad 上清空 run_ids，将后续真实 b 结果删除；`sampling-types::repair_dangling_tool_calls` 也只认连续 ToolResult，补入“Tool execution was cancelled by the user (tool `write_file` was not executed).”

离线调用生产 pure transforms 确认：stripped ids=[b]、synthetic inserted=1，真实成功正文被替换为未执行/用户取消。原始 Timeline 事件仍在；损坏的是模型可见 Surface/其后持久化的 IntegrityRepair 投影，不能表述成原始审计记录被删除。模型可能据此错误重复执行，优先级高于单纯 400。

即使绕过 repair，portable projector 在 `conversation.rs:2025` 同样只扫描连续 ToolResult：全量 portable 重建会丢弃 b 调用和结果；prefix 停在 assistant 之后时，`portable_prefix_end` 只跨过 ToolResult/Reasoning，在 Model SkillLoad 停止，可产生只有 b output 没有 b call 的请求。后两者是 adapter 反例；正常 request builder 可能先触发前述有损 repair，因此不声称生产一定发出该孤立 output。

修复方向：按完整工具批次处理透明 Runtime 元数据，让 Timeline 修复、portable prefix、投影使用一致边界；保留真实结果和已执行事实。不要只补 reasoning 字段掩盖该问题。

## D. Native 分段插话（次级边界，生产可达性未确认）

fixture：native assistant calls[a,b] → result a → AgentMessage → result b。`request_segments` 把 assistant 放在 native 段；后续 defer_agent_messages_after_tool_batch 仅看到中性结果段，不知道还有 b 待结束。三协议都在 b 结果之前插入 synthetic receive 调用。此项升级了上一轮 backlog 的“待验证 adapter 行为”，但没有证明生产会在两个结果之间注入 AgentMessage。

已核对当前常规路径：通知消费发生在采样安全边界；用户 interjection 在整批工具后 drain，并检查 active tool。不能将该构造反例扩大成任意多工具请求都会触发。后续批次修复可用此 fixture 验证 native/neutral 接缝。

## 未确认或已排除

- 上轮 Responses User SkillLoad 确认仍只生成一条 user 上下文，不生成模拟调用。
- Model SkillLoad adapter 不重复加载正文；问题 C 在事实边界和 repair，不在重复输出分支。
- Messages 同样存在 synthetic ToolUse 无 thinking 的形状，但本次没有对应 provider 的拒绝证据，不单独宣称已复现 thinking 400。
- 分类器已经识别截图中的精确 400；加大重试数不能改变上述请求结构。
- 不使用此前 Atlas 的空 scoped search 结果推断函数不存在；最终结论以当前源码和执行探针为准。

## 后续优先级

先独立修 C 的真实结果保全，再修 A/B 的宿主事实和 reasoning 恢复一致性。D 作为批次边界的额外验收反例。当前审计不修改生产实现。
