# 上下文与 token 计数复审

## 口径与调用链

| 数值 | 当前来源/入口 | 含义与用途 |
| --- | --- | --- |
| 累计消耗 | sampling_usage_sink → durable AttemptUsage → UsageLedger | 各次已准入 attempt 的完整 input + output，包含重试、失败、辅助调用及子任务归属；不是上下文占用。 |
| 当前上下文压力 | ChatState projected_tokens | 当前有效请求估值加同路由 provider 校准开销；供状态栏、压缩门槛与发送前硬窗口检查。 |
| 原 Surface 估值 | estimate_conversation_tokens / estimate_item_tokens | 文本 bytes/4、图片固定估值，含原始 reasoning；不等于 portable 请求，也不能直接拿来做 provider anchor 的差分。 |
| 请求估值 | estimate_request_input_tokens | 有效消息/native fragments + 工具 schema + tool choice + JSON 输出 schema；图片/Goal/历史 reasoning 已按请求投影。 |
| 压缩来源估值 | summary source estimate_source callback | 实际发送摘要来源的投影估值；不包含被保留的 tail，不是累计消费或压缩后总压力。 |
| Goal / 子任务输出预算 | Goal settlement / clamp_task_model_request | 消费许可和最大输出额度；不取代上下文窗口准入，也不能用剩余输出额度判断输入是否超窗。 |
| cache hit | cached_read_tokens / cache_read_known_input_tokens | 只对已报告 read 的输入取分母；write 不算命中，缺失不等于零。 |

核心文件：`chat-state/src/actor/{state,mutations,request_builder}.rs`、`chat-state/src/usage.rs`、`shell/src/session/actor/turn/{mod,sampling}.rs`、`shell/src/session/actor/{compaction,updates,run_loop}.rs`、`pager/src/app/acp_handler/mod.rs`、`pager/src/views/agent_status.rs`。源码路径均以 `crates/codegen/` 为根。

## 当前状态变化

| 操作 | 基线/投影行为 | 审查依据 |
| --- | --- | --- |
| 冷恢复/fork 新 native lane | portable 历史初始化；不带入旧 provider 校准，原始 reasoning 保留 | ChatState::from_timeline、rebase_context_pressure_to_portable；前轮冷恢复/rewind 回归。 |
| 普通追加、Control/Memory/Skill | durable 应用后重算有效请求坐标；Memory reminder 先持久化，非临时漏算项 | commit/apply 与 request_builder 起始路径。 |
| route replacement / rewind | 成功后清校准、重建 portable lane；失败 rewind 不发 reset | ReplaceSamplingRoute、rewind_durably。 |
| 普通参数更新 | 保留路由校准；图片 route identity 变化则清图片选择并重算 | UpdateSamplingConfig。 |
| compaction/prune/image shadow | 同路由保留有效校准；按 replacement 后有效历史重算 | finish_surface_replacement / refresh_context_pressure。 |
| request build | 完整估计替换上一次 envelope，重复 build 不累加 | request_builder 末尾与 apply_request_projection。 |
| native reset / portable reasoning | 即刻重算，而非等下一次 build | ResetContinuation / EnablePortableReasoning。 |
| provider response | 仅接纳的候选更新 context anchor；各 attempt 消耗已单独结算 | record_response_token_usage 与 sampling_usage_sink。 |
| 取消/重试/晚到结果 | 计费按原 attempt、unknown 保持 incomplete；取消不接纳候选工具/正文 | AttemptUsage/Goal settlement 及 sampler cancellation 现有测试。 |

本轮发现 Goal 作用域与图片缓存生命周期耦合，见下方；不能因此把上表视为所有组合已实测通过。

## 请求到展示

ChatState 的 ContextPressureUpdated 经 run_loop 传给 transient `grow/contextPressure` 通知，`totalTokens` 在此消息上表示当前压力。累计使用经独立 transient `grow/sessionUsage` snapshot 传递。Pager 排除 replay usage，并对当前窗口回退的累计快照做过滤；状态栏累计 input+output 与 context used/window 使用不同来源。`/usage` 另展示 lifetime 和 resume segments。

Chat/Responses/Messages 的 usage 归一化保留 cache 字段 presence；Messages 仅在缓存桶足够完整时重建 full input，省略 delta 沿用已知值，显式零覆盖。未知总量不凭估值补成真实账单。上下文锚点使用 response total 的坐标，累计账本使用归一化 input+output；两者不能互换。

这部分证据为源码与已有协议/ACP fixtures，本轮没有调用真实在线 provider。状态栏 cache 百分比是已观测输入的缓存命中统计，既不是上下文空闲比例，也不能证明当前请求未超窗。

## 已确认的直接反例

`ImageBudgetSelection.active_goal` 既用于图片缓存域也被 refresh_context_pressure 用作 Goal 请求作用域。ReplaceSamplingRoute 或 image route 更新置 image_budget=None 后，重算传入 active=None，project_conversation_for_goal_scope 会将包括当前活动指令在内的所有 Goal directive shadow。下一次 build 传回原活动 tag 后，该长指令恢复到请求，造成切换后瞬时低估和计数回跳。两项实测反例分别为 36 vs 46,026 和 2,036 vs 48,026；原始输出见 goal-pressure-before.txt。

修复迁移该已有作用域字段到请求状态，图片缓存只拥有图片选择；不保留第二份 tag，不以猜测历史中最后一个 tag 代替当前授权范围。描述模式依赖当前路由能力，不盲目跨路由继承。

## 保留的策略限制与缺口

1. **provider 校准不是无条件权威。** 低于当前本地有效估值的 anchor 被明确拒绝，且现有回归要求如此。这能防明显低报，但 bytes/4 和图片 765 不是模型 tokenizer；真实更小的 provider 计数也可能被忽略，保守偏高可以造成提早压缩/拒绝。需要真实协议证据与契约决定，不能把更改为 signed 校准混入本轮。
2. **普通请求只检查已知 input 压力 > window。** SkillLoad 路径另检查 input + output reserve + 256。普通请求边界 input==window 可通过，且不同 provider 对 output reserve 的约束可能不同。现有规范仅承诺已知 input 超窗不发送；若要统一总预算，需要先明确各 route 的上下文/输出能力语义。此处是可观察策略差异，不宣称已发生 provider 故障。
3. **heuristic 不是精确上界。** 文本按 UTF-8 bytes/4、图片固定估值，工具协议包装与视觉分块存在模型差异；新路由/首次请求无有效校准时尤为明显。native opaque 片段也按其现有估值方法处理。
4. **resume 到 UI 的整体组合缺口。** 已有账本恢复、resume segments、ACP 快照过滤和 status formatting 测试；尚未证明一条完整的 restored ledger → 首次 snapshot → Pager 状态栏及 /usage → 迟到旧 snapshot 的端到端路径。本轮未将局部 fixture 冒充整体验证。
5. **异步 UI 的瞬态。** 压力通知和模型控制通知来自不同转发边界，审查需区分短暂不同步与最终状态错误；不能仅凭一帧红色 used/window 就断定累计用量被当成上下文。

不把这些待决项混入当前字段归属修复；后续验证应围绕具体反例独立建立 change。

冷恢复在首个请求构建前仍没有调用者提供的活动 Goal 参数，本轮不从历史最后一个 tag 猜测授权范围。该阶段的 Goal scope 初始化是另外的集成核对点；本次明确修复的是已完成请求作用域在图片缓存失效时被意外遗忘。
