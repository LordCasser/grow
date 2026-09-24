# model-sampling Specification

## Purpose
定义上层会话与 Provider 流协议之间的采样边界。覆盖三类后端流的统一事件转换，以及通过 Sampler actor 管理请求并发、重试和取消的职责，不承诺所有 Provider 的行为完全相同。

## Requirements

### Requirement: Normalized provider events
采样层 SHALL 将 Chat Completions、Responses、Messages 流转换为统一 SamplingEvent。

#### Scenario: 不同 Provider 后端
- **WHEN** 调用已配置后端的流转换入口
- **THEN** 上层通过统一事件处理响应，而不直接消费每种协议原始 chunk。

证据：`crates/codegen/sampler/src/lib.rs` — `SamplingEvent`。

### Requirement: Request lifecycle management
SamplerHandle SHALL 通过 actor 管理并发请求、重试与取消。

#### Scenario: 取消采样
- **WHEN** 调用方取消某次请求
- **THEN** 请求任务通过取消机制终止，并使用请求事件协调生命周期。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `CancellationToken`。

### Requirement: Cached web content follows current model budget
web_fetch SHALL 对缓存命中的文本应用当前调用的模型上下文预算，保留完整缓存供后续调用使用。

#### Scenario: 大窗口切换到小窗口
- **WHEN** 页面完整文本已缓存且模型窗口缩小后再次获取同一 URL
- **THEN** 输出按新预算裁剪，存在会话目录时保存完整文本并提供恢复路径。

#### Scenario: 再次放大窗口
- **WHEN** 小窗口调用已生成裁剪预览后，大窗口调用命中同一缓存
- **THEN** 返回符合大窗口预算的完整内容，缓存不包含前次调用的裁剪或路径。

### Requirement: Recap sampling uses one configuration snapshot
Session recap SHALL 从同一准备配置生成 client、显式模型和 context_window 预算；准备完成后的配置切换 SHALL 不把新模型或预算拼入旧 endpoint/backend 的请求。

#### Scenario: 准备后切换模型
- **WHEN** recap 已准备配置且会话切换到另一模型或 endpoint
- **THEN** 此 recap 请求仍使用准备配置的 endpoint/backend/model/window，后续 recap 才使用新配置。

### Requirement: Side questions retain their prepared sampling route
/btw SHALL 从同一准备配置构建 client 与显式请求模型；生成和既有重试期间的会话配置变化 SHALL 不将新模型拼入旧 endpoint/backend。

#### Scenario: Client 准备后切换会话模型
- **WHEN** /btw 准备完成后会话切换到其他 endpoint 或模型
- **THEN** 该请求仍使用准备时的 endpoint/backend/model，会话新配置保持。

### Requirement: Suggestion generation follows receiver lifetime
AI Suggest 与 Prompt Suggest 后台任务 SHALL 在结果接收端关闭时停止等待并丢弃生成 future，释放任务 activity；关闭已发生时 SHALL 优先处理关闭而不 poll 生成 future。正常完成 SHALL 交付结果。

#### Scenario: 请求超时或被丢弃
- **WHEN** 接收端在生成过程中关闭
- **THEN** 生成 future 被丢弃，已有 Sideband 取消清理生效。

#### Scenario: 开始前关闭
- **WHEN** 后台任务首次运行前接收端已关闭
- **THEN** 不开始生成。

#### Scenario: 正常交付
- **WHEN** 生成完成且接收端仍存在
- **THEN** 接收端取得生成结果。

### Requirement: Image description provider work respects recovery deadline
Auxiliary image-description provider polling SHALL use an absolute deadline no later than the existing image recovery deadline. Time spent preparing a group SHALL NOT extend that deadline. Durable preparation and terminal persistence are outside this provider-time guarantee.

#### Scenario: Preparation consumes the remaining recovery budget
- **WHEN** group preparation finishes after the recovery deadline
- **THEN** its provider request is not polled and recovery follows its timeout failure path without installing an incomplete image shadow

#### Scenario: Preparation consumes part of the remaining budget
- **WHEN** a group reaches provider entry before the recovery deadline
- **THEN** provider polling is bounded by the earlier of that deadline and its per-call timeout

### Requirement: Image description recovery batches durable lookups
A recovery pass SHALL reuse one validated durable-history load for its uncached image-description queries. It SHALL preserve exact source revision, Surface identity, prompt and completed-result provenance checks, including integrity validation of other Sidebands required by the parent Timeline. It SHALL NOT retain the full loaded history across auxiliary provider requests.

#### Scenario: Several groups need durable reuse
- **WHEN** a recovery pass has multiple uncached image groups
- **THEN** their durable lookups share one history load and each query receives only its own matching completed result

#### Scenario: Historical Sideband integrity fails
- **WHEN** required Sideband validation fails during batch lookup
- **THEN** no result is accepted from that failed snapshot and existing description failure or provider fallback handling applies

#### Scenario: Every group is cached
- **WHEN** all group descriptions are available in the session cache
- **THEN** the pass performs no durable-description history load

### Requirement: Proxy thinking signatures recover without losing portable history
Messages thinking starts MAY omit an initial signature. A subsequent signature delta SHALL supply it normally. A response containing unsigned thinking SHALL retain visible facts but SHALL NOT retain provider-native continuation.

#### Scenario: Signature arrives later
- **WHEN** a thinking start omits signature and a later valid signature delta completes the block
- **THEN** decoding succeeds and the completed signed block remains eligible for native continuation.

#### Scenario: Thinking never receives a signature
- **WHEN** a complete response has an unsigned thinking block
- **THEN** visible reasoning, text and tool facts remain available and its native continuation is discarded.

### Requirement: Missing signature recovery is bounded by native request state
A missing-field signature serialization failure on a request with native continuation SHALL use the acknowledged continuation reset and retry with portable context. Other serialization errors and portable requests SHALL NOT qualify for this recovery.

#### Scenario: Native signature mismatch
- **WHEN** a native-bearing request fails with missing field signature
- **THEN** native state is cleared and the request is retried without opaque signatures while retaining portable conversation facts.

#### Scenario: Portable retry fails
- **WHEN** the portable request encounters the same missing signature error
- **THEN** it terminates instead of repeating continuation recovery.

### Requirement: Sampling authentication logs omit credential fragments
Sampling client construction/request events and sampling request spans SHALL describe authentication using type and presence metadata without raw credential prefixes, suffixes or complete values. This SHALL NOT alter outgoing authentication headers or the independent 401 attribution callback.

#### Scenario: Short bearer or API key
- **WHEN** a request is built with a short bearer token or x-api-key
- **THEN** sampling event/span logs contain no credential value while the built request retains the configured authentication header.

### Requirement: Image normalization uses the bounded compute path

Image normalization SHALL run through its cancellation-safe worker admission without an inactive process-wide normalization cache or its unused remote activation flag. Image format conversion, integrity checks, original-content fallback and attachment metadata SHALL retain their existing behavior. Full pixel decode SHALL reject source images above 50,000,000 pixels before allocating their pixel buffer. One normalization batch SHALL admit at most 25 images and 80,000,000 bytes of encoded image data before awaiting compute. Concurrent normalization batches SHALL reserve at most 160,000,000 encoded bytes process-wide; a batch that cannot reserve capacity SHALL promptly drop its images instead of retaining them as unbounded waiters. A running blocking worker SHALL retain its batch reservation after caller cancellation until that work exits. Drop outcomes SHALL remain attributable to individual input indexes but SHALL be grouped by identical reason before rendering; one normalization batch SHALL produce at most one model reminder and one `ImageDropped` update, with each distinct reason rendered once and all affected indexes listed in stable input order.

#### Scenario: Normalize repeated attachments

- **WHEN** attachments with identical content are admitted
- **THEN** each uses the supported normalization path and produces the same valid content and per-attachment metadata without consulting a disabled cache.

#### Scenario: Cancel a normalization waiter

- **WHEN** a caller is cancelled after blocking normalization has started
- **THEN** the running worker retains both compute admission and its encoded-byte reservation until its actual work completes.

#### Scenario: Re-encoding cannot meet the bound

- **WHEN** normalization cannot produce an encoding under its byte limit for an admitted image
- **THEN** the original attachment and its indexed fallback notice remain available.

#### Scenario: Several images fail for the same reason

- **WHEN** one normalization batch drops multiple images for an identical integrity, dimension, pixel-count or admission reason
- **THEN** the model reminder and `ImageDropped` notes contain one summary line for that reason with every affected image index, and the client receives one NOTICE block rather than one repeated sentence per image.

#### Scenario: Images fail for different reasons

- **WHEN** one normalization batch drops images for more than one reason
- **THEN** each distinct reason appears on one line in first-occurrence order, indexes within each line preserve input order, and the user-attachment and tool-result paths use the same summaries.

#### Scenario: Too many images or encoded bytes

- **WHEN** a normalization batch contains more than 25 images or its encoded payload exceeds 80,000,000 bytes
- **THEN** excess images are released before compute starts, admitted images remain in input order, and excess indexes receive explicit dropped-image outcomes.

#### Scenario: Concurrent batches exhaust encoded-byte capacity

- **WHEN** a new batch would raise live normalization input reservations above 160,000,000 bytes
- **THEN** its admitted images are promptly dropped with indexed capacity reasons; no encoded payload waits uncharged for the compute worker.

#### Scenario: Source exceeds client decode budget

- **WHEN** an image above 50,000,000 source pixels requires re-encoding
- **THEN** the normalizer drops it before full decode even if it is below the provider's larger persisted-image validity ceiling.

### Requirement: Session image descriptions retain original media

Grow SHALL retain original image information as immutable Timeline evidence. Sampling SHALL send original images for an unmarked canonical provider/model pair. Only a confirmed unsupported-image failure SHALL mark that pair in the current session and trigger a durable `ImageProjection`; unrelated request failures SHALL NOT do so. A successful description or OCR projection SHALL retain original images alongside reusable text in the current Surface. When no textual fallback can be produced after a confirmed rejection, an acknowledged unsupported-image projection SHALL remove the unresolved images from the current Surface and leave the canonical replacement text without altering the original Timeline message.

#### Scenario: First request for a model

- **WHEN** a session sends an image to an unmarked provider/model pair
- **THEN** the request includes original image content rather than a preemptive description or deletion.

#### Scenario: Model rejects image input

- **WHEN** the primary model explicitly rejects a request that contains image input
- **THEN** the session records that provider/model pair, commits one exact-revision image projection, and retries only after the projection is durably acknowledged.

#### Scenario: GLM-style multimodal rejection

- **WHEN** an image-bearing request receives HTTP 400 with `InvalidParameter: glm-5.2 is not a multimodal model`
- **THEN** the shared classifier treats it as an unconditional image-input rejection rather than an ordinary terminal request failure.

#### Scenario: Switch to a different model

- **WHEN** sampling switches to an unmarked provider/model pair after another pair used a successful description or OCR projection
- **THEN** the first request to the new pair can use the retained original images, while returning to a marked pair selects the reusable text.

#### Scenario: Switch after an unsupported removal projection

- **WHEN** unresolved images were durably removed from the current Surface and sampling later switches models
- **THEN** request assembly does not resurrect images from immutable Timeline evidence; retrying the original media requires an explicit rewind or new attachment.

#### Scenario: Unrelated image validation failure

- **WHEN** a 400 reports malformed bytes, size, dimensions, format, transparency or policy rather than unconditional model capability
- **THEN** Grow does not mark the pair text-only and does not delete images through the unsupported-model projection.

### Requirement: Visual auxiliary and local OCR fallback

For explicitly unsupported image input, Grow SHALL use an available configured visual auxiliary model to describe images and show `当前模型不支持多模态，调用视觉辅助LLM处理中...`. If the auxiliary model is absent or fails, Grow SHALL show `视觉辅助模型未配置或者调用失败，使用OCR处理中...` and attempt local OCR. A successful description or OCR result SHALL be retained and reused for marked models. Every still-unresolved image group SHALL instead receive a typed unsupported-model projection whose exact replacement is `当前模型不支持多模态，图片已经被删除`. Description, OCR and removal shadows for one Surface revision SHALL commit atomically before the primary request is rebuilt.

#### Scenario: Auxiliary description succeeds

- **WHEN** a configured visual auxiliary model produces a nonempty description
- **THEN** Grow retains that description and retries the primary request with text after the projection ACK.

#### Scenario: Auxiliary unavailable

- **WHEN** the visual auxiliary route is unconfigured or fails
- **THEN** Grow displays the OCR status and attempts local OCR, retaining a successful nonempty result in the description field.

#### Scenario: Both fallbacks fail

- **WHEN** neither visual description nor local OCR can provide text for an image group after the primary model explicitly rejects images
- **THEN** Grow durably replaces that group in the current Surface with one `当前模型不支持多模态，图片已经被删除`, removes its raw image parts and retries the primary request without those images.

#### Scenario: Mixed fallback outcomes

- **WHEN** one rejected request contains groups that are described successfully and groups that remain unresolved
- **THEN** one exact-revision projection atomically attaches descriptions to the successful groups and removes only the unresolved groups, without emitting repeated per-image projection notices.

#### Scenario: Image fallback fails without damaging the session

- **WHEN** unsupported-image removal is acknowledged and the user later sends a text-only follow-up
- **THEN** the new request uses the repaired Surface, contains none of the removed historical images and does not repeat the same capability 400 merely because the raw Timeline evidence still exists.

#### Scenario: Image projection cannot be committed

- **WHEN** projection validation, durable write or acknowledgement fails
- **THEN** Grow does not resubmit an in-memory-only lossy request, reports a typed projection failure, and preserves the original Timeline evidence for exact retry or explicit recovery.

### Requirement: Malformed completed tool arguments recover without execution

Chat Completions、Responses 和 Messages 中结构及身份有效且完整结束的响应若含非法工具 JSON，sampling SHALL 拒收整个候选，并依照统一 attempt 恢复条件重采样最后有效输入。无效调用及有效 sibling SHALL NOT 成为可执行工具或被接纳的 native continuation。该恢复 SHALL NOT 使通用 serialization、明确的 incomplete status 或身份冲突自动可重试；也 SHALL NOT 绕过不可撤销输出、结算或预算保护。

#### Scenario: Bad arguments followed by valid output
- **WHEN** 第一次完整候选包含非法工具 JSON，输出可废弃且统一恢复条件满足，下一 attempt 有效
- **THEN** 发出 Retrying 诊断并有序废弃旧 attempt，仅接纳有效候选，TUI 保持运行活动，不通过终端失败暂停 Goal。

#### Scenario: Repeated invalid generation
- **WHEN** 非法工具 JSON 持续出现
- **THEN** 按三次总 attempt、配置分类上限及逻辑采样剩余总额度中的更严格限制停止；显式禁用自动恢复时不重采样。

#### Scenario: Accounting and evidence before retry
- **WHEN** 被拒收 attempt 具有已知或未知消费
- **THEN** 既有证据和所有适用账本结算先确认，失败或 Goal 准入关闭阻止下一 attempt。

#### Scenario: Cancellation during recovery
- **WHEN** 下一 attempt 前发生请求取消
- **THEN** 不再发起 provider 请求。

### Requirement: Sampling recovery uses typed attempt facts

采样 SHALL 区分缺少完成证据的流中断、确定性协议违例、完整但无效的生成、远端传输/服务失败和本地生命周期失败，并保留实际结束方式。重新采样资格 SHALL 同时依赖错误事实、输出交付状态、未接纳状态、外部副作用安全性、结算确认、所有者准入及恢复额度；内部决策 SHALL NOT 通过序列化诊断文本重建这些事实。

#### Scenario: Provider stream ends before completion evidence
- **WHEN** Chat 缺少 choice finish_reason、Responses 缺少 terminal response 或 Messages 缺少完整结束序列就到达 EOF，且此前不存在确定性协议违例
- **THEN** 候选被拒收并归类为不完整流，在输出可丢弃、结算确认、无不明外部副作用且准入/额度允许时重试原有效输入。

#### Scenario: Idle timeout before completion
- **WHEN** 未完成响应发生 idle timeout
- **THEN** 与不完整流共用上述恢复判定，超时不会把半截工具调用变为可执行结果。

#### Scenario: Protocol conflict precedes EOF
- **WHEN** 流已出现 response/tool 身份冲突、非法索引或矛盾状态后结束
- **THEN** 保留确定性协议违例并停止，不因尾部 EOF 降格为可自动恢复的流中断。

#### Scenario: Local stream producer disappears
- **WHEN** Grow 内部事件生产者结束且没有约定的终态
- **THEN** 报告本地生命周期失败，不能作为远端 EOF 自动重发。

#### Scenario: Missing optional usage after valid completion
- **WHEN** 已取得合法完成证据且候选有效，但有界尾部等待未取得完整 usage
- **THEN** 不因缺少 usage 重新生成正文，按未知用量结算规则和响应接纳规则处理。

#### Scenario: Legitimate semantic terminal result
- **WHEN** provider 明确返回合法 length、content filter、pause 或上下文窗口终止结果
- **THEN** 保留原始语义与完整性校验结果，由 session 执行既有继续、压缩或停止逻辑，不作为缺失终态原地重采样。

#### Scenario: Uncertain provider-side effects
- **WHEN** 请求可能执行有副作用的 provider 托管工具且无法证明重放安全
- **THEN** 自动重采样被拒绝，即使 Grow 本地工具尚未执行且输出可撤销。

### Requirement: Every provider attempt settles all applicable usage before readmission

主/子 agent 模型步骤的每次真实 provider attempt SHALL 具有独立于 Goal 存在与否的归属，并向所有适用的现有消费账本结算，包括最终未被接纳的候选。结算 SHALL 按归属幂等并等待确认；成功响应接纳 SHALL NOT 再累计已结算的消费。未知用量 SHALL 保留未知状态，精确预算下关闭准入。重试 SHALL 重新检查剩余子任务输出额度。

#### Scenario: Rejected attempt followed by success
- **WHEN** 第一次调用产生已知消费但候选被废弃，第二次调用成功
- **THEN** prompt/session/model、Goal 和子任务输出账本中所有适用账本包含两次消费各一次，上下文锚点仅由被接纳响应更新。

#### Scenario: No active Goal
- **WHEN** 没有 Goal 的请求在内部恢复前产生消费
- **THEN** 普通账本和子任务预算仍按 attempt 结算，不因 Goal scope 为空而跳过。

#### Scenario: Partial or lost settlement acknowledgment
- **WHEN** 部分账本已写入但其他结算失败或确认丢失
- **THEN** 禁止下一次 provider 准入，仅核对或幂等补交原 attempt 的结算，不能重复计费或为恢复账本重新推理。

#### Scenario: Unknown spend with an exact budget
- **WHEN** 已开始的 attempt 无法取得完整用量且受精确 Goal 或子任务预算约束
- **THEN** 记录用量不完整并关闭后续准入，不把未知消费当零。

#### Scenario: Unknown spend without an exact budget
- **WHEN** 用量不完整但没有精确预算约束且其他恢复条件均满足
- **THEN** 确认记录未知状态后可继续有界恢复，已知下界不冒充完整用量。

#### Scenario: Remaining output grant shrinks
- **WHEN** 失败 attempt 的已知输出消费减少子任务剩余额度
- **THEN** 下一 attempt 的输出上限不超过新余额，余额耗尽则不派发。

### Requirement: Automatic recovery shares a logical sampling budget

同一个未接纳模型步骤的自动恢复 SHALL 共享总 provider attempt 上限和绝对期限，包含 sampler 重试、doom 重采样及 session 修复后的重提交。分类上限只能收紧总上限。取消、owner epoch 失效、服务端 veto 和预算关闭 SHALL 阻止下一次派发；backoff SHALL 可取消。语义修复 SHALL 由 session 持有，sampler 不修改会话历史。

#### Scenario: Different recovery causes alternate
- **WHEN** 同一步骤依次发生传输失败、非法生成和 native continuation 修复
- **THEN** 每次真实调用都消耗同一总额度和期限，不因切换分支、更新配置或重提交重置。

#### Scenario: Recovery disabled
- **WHEN** 请求显式禁用自动恢复
- **THEN** 仅执行初次准入的调用，任何分类特例都不能额外派发。

#### Scenario: Cancellation during discard or backoff
- **WHEN** 旧 attempt 被关闭之后、下一次 provider poll 之前发生取消或 owner 失效
- **THEN** 不再调用 provider，已有 attempt 的独立结算仍完成或保留待确认状态。

#### Scenario: Request state changes for recovery
- **WHEN** session 确认需要刷新凭据、清空 native continuation、转换图片输入或压缩上下文
- **THEN** session 确认所需状态变化后按新修订提交，并沿用当前逻辑采样剩余恢复额度；半截候选不加入输入。

#### Scenario: Logical deadline expires
- **WHEN** backoff 或 provider polling 到达调用者更早期限或逻辑采样绝对期限
- **THEN** 停止后续模型活动和准入，不重新起表，也不硬中断独立用量结算。

### Requirement: Portable history preserves complete local tool exchanges

Portable 请求投影 SHALL 保留完整、无歧义的本地工具调用和匹配结果，并通过目标 backend 的结构化工具协议表达名称、合法 JSON 对象参数、关联 ID、结果正文及预算允许的图片。默认投影 SHALL 移除旧 provider reasoning、签名、加密数据、输出 item identity/status 和模型诊断；若当前路由以明确的协议错误要求回传 Responses `reasoning_text` 或 Chat Completions `reasoning_content`，则该路由 SHALL 仅回放 Surface 中既有的可见 reasoning 文本；Chat 历史 assistant 没有该文本时 SHALL 编码空字符串，仍 SHALL 移除 opaque identity、签名、加密内容及状态。投影 SHALL NOT 将历史调用作为新的执行请求。原始 Timeline 和既有隔离事实保持不变。

#### Scenario: Tool attachments include eviction text
- **WHEN** 工具结果附件同时含图片与图片预算产生的文本，或只剩替换文本
- **THEN** 三协议在 live 和 portable 请求中都保留附件顺序及文本，不因附件位于 images 字段而丢弃非图片内容。

#### Scenario: Restore or switch provider
- **WHEN** 会话恢复或切换模型/backend 后中性历史包含完整工具往返
- **THEN** Chat Completions、Responses、Messages 请求分别保留配对的工具协议和内容，默认不包含被撤销的 native reasoning，切回原模型也不复活 native；仅当前 Responses 或 Chat Completions 路由明确要求时可回放无 opaque 字段的可见 reasoning。

#### Scenario: Ambiguous or incomplete history
- **WHEN** portable 区域包含未配对、重复或无效工具记录
- **THEN** 不输出悬空调用、孤立结果或歧义配对，不伪造工具结果或修补非法 JSON；原始持久化证据不变。

#### Scenario: Valid same-route native continuation
- **WHEN** 请求仍有当前 epoch 的有效 native span
- **THEN** 该 span 继续使用完整原生内容，portable 处理不删掉其 thinking 或重复生成工具调用。

#### Scenario: Target route requires reasoning text replay
- **WHEN** Responses 路由对含 portable 工具历史的请求返回明确 400，声明 thinking mode 必须回传 `reasoning_text`
- **THEN** 系统在当前路由内启用可见 reasoning 回放，确认投影状态后按同一 logical sampling 余额重建请求，reasoning、function call 与结果各保留一次。

#### Scenario: Replayed portable reasoning remains narrow
- **WHEN** 兼容回放已为当前 Responses 或 Chat Completions 路由启用，随后发生 native reset、相同路由参数更新或真正的 route 替换
- **THEN** 前两者保留当前路由兼容状态，真正 route 替换清除它；Messages、未学习路由以及与已学习 backend 不匹配的 wire 转换不接收该 portable reasoning。

#### Scenario: Chat route requires reasoning content after a switch
- **WHEN** 切换后的 Chat Completions 路由明确要求历史 assistant 回传 `reasoning_content`
- **THEN** 确认启用后，每条 assistant 使用紧邻它的已有可见 reasoning 按顺序编码（包括普通正文），无文本时使用空字符串；保留完整工具调用、结果和附件，不重放工具执行。

#### Scenario: Chat reasoning stays within its assistant boundary
- **WHEN** 历史含多段 reasoning、不同 assistant、User/System/ToolResult 边界或有效 native span
- **THEN** 可见 reasoning 不跨越无关边界绑定，native 原文保持且不重复，缺 reasoning 字段时仅补空字符串；Timeline 不改写。

### Requirement: Portable boundaries keep tool exchanges together

Portable prefix 与 live suffix 的切点 SHALL NOT 将同一完整工具往返拆成被删除的调用和孤立结果。生成请求时 SHALL 在现有消息/native span 边界内闭合连续工具结果；wire 转换、请求 token 估算及投影证据 SHALL 使用一致的范围。

#### Scenario: Unsigned response precedes tool execution
- **WHEN** 完整 Messages 响应的 unsigned thinking 使 native 撤下，assistant 持久化后工具执行并追加结果
- **THEN** 下一请求保留该调用和结果各一次，不带无效 thinking/signature，已执行工具不重放。

#### Scenario: Several results straddle the prefix
- **WHEN** 同一批工具的多个结果分处 prefix 两侧
- **THEN** 投影保留完整配对及图片，估算与实际投影一致，不吞并后续 native span 或新消息。

#### Scenario: Normal final answer
- **WHEN** 普通模型请求收到完整、合法的正常 provider 终止和可见回复，且没有待执行调用或独立宿主继续理由
- **THEN** 正常结束本 Turn，不要求 FinishTurn；末尾标点和正文是否像行动预告不参与完成判定。

### Requirement: Provider termination remains distinct from host control

三 backend SHALL 保存实际收到的原生 terminal 信息，并保留独立的中性终止语义。工具调用的存在 SHALL NOT 覆写拒绝、截断或其他 provider 原因。provider 结束仅表示该响应结束，SHALL NOT 自动完成 Goal。原始终止事实 SHALL 与所属 request/attempt 关联并在候选拒收后仍可追溯。

#### Scenario: Natural stop without a completion tool
- **WHEN** Chat stop、Messages end_turn 或 Responses completed 返回正常可见回复且无业务调用
- **THEN** 普通 Turn 可正常结束，不因缺少 FinishTurn 再次采样，原始 backend 字段保留。

#### Scenario: Natural stop with complete calls
- **WHEN** 合法响应带完整可执行调用
- **THEN** 宿主按工具协议执行后续步骤，原始终止仍保持 provider 给出的原因。

#### Scenario: Refusal after complete tool output
- **WHEN** provider 拒绝响应同时包含完整工具调用
- **THEN** 记录拒绝及调用事实，生成明确未执行的配对结果，不执行该批业务工具，不把拒绝转换成普通完成或无声明恢复。

#### Scenario: Terminal received but candidate rejected
- **WHEN** 已观察到 provider terminal，随后参数/协议校验失败或宿主拒收候选
- **THEN** attempt evidence 同时保留已观察的原生 terminal 和宿主拒收/失败结果，不能将 terminal 改成未收到。

#### Scenario: No provider terminal received
- **WHEN** EOF、传输故障或取消发生且未收到原生终止
- **THEN** 不合成 provider 成功终止；按既有失败/恢复预算处理，宿主生命周期单独关闭。

#### Scenario: Provider switch after a response
- **WHEN** 响应 A 结束后切换到端点或模型 B
- **THEN** A 的 terminal 仍绑定 A 的 request/attempt 和原始 backend，后续宿主终态不能用 B 的配置覆盖它。

### Requirement: Provider-required portable reasoning recovery is bounded

系统 SHALL 将明确的 Responses `reasoning_text` 或 Chat Completions `reasoning_content` 回传拒绝表示为类型化 attempt 事实。兼容状态只有在拒绝指向当前 backend、存在会改变 wire 的有效历史且当前路由尚未启用时才可改变；自动重提交 SHALL 使用同一 logical sampling 的剩余 attempt 上限与绝对期限。

#### Scenario: First explicit rejection enables replay
- **WHEN** 当前请求因缺少要求的 reasoning 字段首次被明确拒绝，且存在可改变的有效历史与恢复余额
- **THEN** ChatState 确认启用当前路由投影模式后静默重提交，不把失败候选加入 Surface。

#### Scenario: Repeated rejection does not loop
- **WHEN** 回放模式已经启用后端点仍返回相同拒绝，或不存在可改变的有效历史（Responses 仍要求完整工具往返前的非空 reasoning）
- **THEN** 系统不再次声明状态已改变，不重置 logical sampling 预算，并按既有其他恢复或终态路径处理。

#### Scenario: Chat history has no visible reasoning
- **WHEN** Chat 历史 assistant 没有可见 reasoning，端点明确拒绝缺失 `reasoning_content`
- **THEN** 当前 Chat 路由可确认一次空字段编码恢复，不能为满足字段要求虚构 reasoning 正文。

#### Scenario: Rejection identifies a different backend
- **WHEN** 错误要求的 reasoning 字段属于另一 backend，或错误不同时满足 400、thinking mode 与明确回传要求
- **THEN** 不启用当前路由的 reasoning 兼容状态。

验证入口：`sampling-types/src/conversation.rs` 的 wire 投影、`sampling-types/src/error.rs` 的分类、`chat-state/src/actor/tests.rs` 的路由状态测试与 `shell/src/session/actor/turn/sampling.rs` 的恢复测试。

### Requirement: Messages tool identity encoding preserves distinct exchanges
Messages 请求 SHALL 将中性工具调用 ID 与结果关联 ID 一致转换为 ASCII 字母、数字、下划线或连字符组成的非空有界 ID。请求中不同原始身份 SHALL NOT 因编码而碰撞。有效 native continuation 的原生内容和 ID SHALL 保持不变，生成 ID SHALL 避免与其冲突。编码 SHALL NOT 改写原始 Timeline 或执行身份。

#### Scenario: Portable IDs contain punctuation Unicode or excessive length
- **WHEN** 完整工具往返包含替换标点后会同名的 ID、Unicode 或超出本地编码预算的 ID
- **THEN** wire 保留每个独立调用及对应结果，ID 合法且一一配对，重复构造相同请求产生相同映射。

#### Scenario: Existing IDs overlap generated candidates
- **WHEN** 一个合法或 native ID 与另一个身份的初始编码候选相同
- **THEN** 保留已有身份，为被编码身份选择未占用 ID，并保持调用结果配对。

#### Scenario: Native tool use precedes a neutral result
- **WHEN** 同路由有效 native tool use 后续由中性历史提供结果
- **THEN** 结果引用原生 ID，thinking 签名及原生块保持原样。

证据：crates/codegen/sampling-types/src/conversation.rs 的 build_messages_request 与 portable/native tests。

### Requirement: Sampler shutdown preserves admitted attempt settlement

Sampler 正常关闭 SHALL 先停止新请求准入并向活跃 provider 工作发送取消，再等待所有已准入 request task 完成可用 attempt evidence 和所有适用用量账本的确认结算。正常关闭 SHALL NOT 通过 abort request task 跳过这些独立结算。只有显式有界关闭期限到期后才能强制终止，且该结果 MUST 报告为关闭失败而非成功的 graceful frontier。

#### Scenario: Shutdown while provider work is active

- **WHEN** sampler 正常关闭时已准入 attempt 仍在 provider polling 或取消收尾
- **THEN** sampler 阻止新准入、取消 provider 工作并等待该 attempt 的 evidence 与适用用量结算完成后才报告正常关闭成功。

#### Scenario: Shutdown while settlement acknowledgment is blocked

- **WHEN** provider 工作已经结束或取消，但 evidence 或任一适用用量账本的确认仍在等待
- **THEN** 正常关闭继续等待原 attempt 的确认，不 abort request task、不重复结算，也不把缺失确认当成已完成。

#### Scenario: Forced shutdown deadline expires

- **WHEN** 已准入 request task 在 owner 的明确关闭期限内仍不能完成
- **THEN** owner 强制终止剩余工作并返回关闭失败；调用方不能把该结果当作成功跨越的最终持久化边界。

证据入口：`crates/codegen/sampler/src/actor/mod.rs` 的 actor shutdown 与 `SamplerOwner::shutdown_bounded`，`crates/codegen/shell/src/session/actor/teardown.rs::shutdown_sampler`。

### Requirement: Side questions preserve completed tool evidence from their frozen context

`/btw` SHALL 从一次冻结的已提交主会话上下文构造只读请求，保留其中身份有效、可无歧义配对的已完成工具调用及结果、结果附件和普通 Assistant 正文，包括连续工具交换构成的尾部。未完成调用 SHALL NOT 输出悬空工具协议或伪造执行结果，同批已完成调用 SHALL 保留。Sideband SHALL 不提供工具能力、不改变主模型 Surface，重试 SHALL 复用同一冻结输入。

#### Scenario: Completed tools are the latest context
- **WHEN** `/btw` 冻结上下文的尾部由连续已完成工具交换组成，尚无单独的 Assistant 总结
- **THEN** 发往 provider 的请求包含这些调用、结果和附带正文，随后才是旁路问题。

#### Scenario: Only some parallel calls have completed
- **WHEN** 最新 Assistant 发出多个调用，冻结时只有部分结果已提交
- **THEN** 请求保留已完成调用及对应结果和 Assistant 正文，不包含无结果调用的工具协议，之前的完整交换保持。

#### Scenario: Latest call is still running
- **WHEN** 冻结上下文以未得到任何结果的调用结束
- **THEN** 请求保留其普通 Assistant 正文及之前的完整交换，不声称该调用已完成，也不改变主上下文中的运行中调用。

#### Scenario: Main conversation advances during a retry
- **WHEN** 第一次旁路请求 overload 后，主会话提交了新消息，再发生旁路重试
- **THEN** 重试仍使用首次冻结的内容及来源引用，旁路问题与回答均不追加到主模型 Surface，且没有工具定义或 tool choice。

实现与验证入口：`crates/codegen/shell/src/session/actor/recap.rs` 的 `handle_side_question`；`crates/codegen/shell/src/session/actor/tests/recap_display_only_tests.rs` 的 Sideband wire 回归。

### Requirement: Recap retains completed tool evidence
Recap SHALL 保留冻结输入中可合法配对的已完成工具调用、结果、附件与 Assistant 正文，不因它们位于尾部而删除。未完成调用 SHALL 不输出悬空协议；预算裁剪后的请求仍保持配对。

#### Scenario: Complete and partial tool batches at recap boundary
- **WHEN** recap 在完整或部分完成的工具批次后生成
- **THEN** 请求保留已完成调用及结果与普通正文，排除未完成调用，主 Surface 不变。

#### Scenario: Recap requires budget trimming
- **WHEN** recap 输入超预算而需要去掉较早历史
- **THEN** 最新可保留工具证据进入请求且不存在孤立工具协议。

### Requirement: Subagent cancellation settles admitted attempts before closing the child Timeline

Subagent cancellation SHALL stop new provider admission, cancel active provider work and await the terminal evidence and all applicable usage settlement for every admitted attempt before committing the SubagentResult that closes the child Timeline. Unknown usage SHALL cross the existing durable incomplete-settlement boundary rather than being treated as zero. A cancellation signal alone SHALL NOT authorize child Timeline closure.

#### Scenario: Cancellation overlaps final attempt settlement

- **WHEN** a child is cancelled after its final provider attempt was admitted but that attempt's evidence or an applicable usage settlement acknowledgment is still pending
- **THEN** the cancellation remains in settlement, the child SubagentResult and parent Ended reference remain uncommitted, and the attempt settlement is allowed to reach its durable boundary before child closure.

#### Scenario: Settlement completes after cancellation

- **WHEN** all admitted attempt evidence and known or incomplete usage settlements are durably acknowledged and the child turn reaches its cancellation terminal
- **THEN** the system may commit exactly one SubagentResult(cancelled), followed by the parent Ended fact that references it, and no later attempt event is appended to that child Timeline.

#### Scenario: Settlement or terminal acknowledgment fails

- **WHEN** the child cannot confirm attempt settlement, its turn terminal, or the required persistence frontier within the existing bounded shutdown policy
- **THEN** the system reports a terminalization failure, does not commit a misleading canonical SubagentResult and does not make the lifecycle eligible for resume.

#### Scenario: Cancellation has no admitted provider work

- **WHEN** a child is cancelled before any provider attempt is admitted
- **THEN** the empty settlement frontier is acknowledged through the same terminal path and the canonical cancelled lifecycle may close without a fixed delay.

### Requirement: Runtime agent messages project to complete tool exchanges

有来源的 runtime agent-message context item SHALL 在模型请求中投影为完整、可配对的专用收件工具调用/结果，使用从 receipt 确定的稳定身份。该投影 SHALL NOT 冒充模型主动执行或触发工具 dispatch，不得追加孤立 ToolResult 或篡改其他真实工具的结果。canonical Surface 坐标 SHALL 不因 provider 一对多展开改变。

#### Scenario: Receiving a message during a tool batch
- **WHEN** 消息到达而现有真实工具批次尚未闭合
- **THEN** 消息在安全边界加入请求，完整收件调用/结果对不插断原工具配对。

#### Scenario: Provider switch and portable history
- **WHEN** 含已消费 agent 消息的请求切换现有 provider adapter 或供 Sideband 冻结
- **THEN** 完整正文、来源和稳定身份保留，portable projection 不把它作为孤立结果丢弃，也不执行历史调用。

#### Scenario: Compaction and source permissions
- **WHEN** 对含 runtime agent message 的 Surface 进行 compaction 或权限判定
- **THEN** input_ref 仍指向 canonical 事实，消息不获得 DirectUser/Interjection 权限证据；一对 provider item 不生成两份消费坐标。

### Requirement: Named provider stream errors retain their error facts

Chat Completions, Responses, and Messages SHALL recognize a complete `event:error` SSE frame with nonempty `code` and `message` as a provider error, even when its JSON body lacks a `type` or nested `error` field. Grow SHALL retain the provider code, message, and supplied request ID for diagnosis. Error classification SHALL distinguish confirmed content rejection, throttling, and overload, while unknown codes and malformed ordinary events SHALL NOT gain automatic retry eligibility. Existing attempt settlement, output retraction, admission, tool execution, and shared retry budgets SHALL remain authoritative.

#### Scenario: Content rejection after provisional tool output
- **WHEN** a provider emits a partial tool candidate followed by `event:error` with `InvalidParameter`, a content rejection message, and a request ID
- **THEN** Grow reports the provider facts, settles usage as known or unknown according to actual evidence, rejects the entire candidate, executes no tool, and does not retry that rejection.

#### Scenario: Known transient stream error
- **WHEN** an `event:error` frame carries a confirmed throttling or service overload code
- **THEN** Grow uses the existing 429 or overload classification, and any retry remains bounded by attempt safety and the shared budget.

#### Scenario: Missing fields or unknown code
- **WHEN** a non-error SSE frame lacks a required event type, a named error lacks required fields, or a named error has an unknown provider code
- **THEN** missing-field frames remain protocol failures and unknown provider errors preserve their code/message without being assumed transient.

### Requirement: Cross-segment tool IDs identify one exchange

在一个 provider 请求中，工具关联 ID SHALL 只标识一个调用。原生 span 的工具调用 ID 若被 span 外另一个中性 Assistant 调用复用，请求投影 SHALL 按完整中性历史重新投影，并拒绝该 ID 的歧义工具协议；此回退 SHALL NOT 改写 Timeline 或执行工具。原生调用的持久化镜像位于同一 span 内，及其后续中性结果，SHALL 仍可保留原生 continuation。

#### Scenario: Neutral and native exchanges reuse an ID
- **WHEN** a neutral assistant call outside a native span reuses the ID of a distinct native tool use inside that span
- **THEN** request projection discards native continuation for that request and omits the ambiguous call/result protocol through the full portable projection while preserving other conversation facts; no provider request contains both owners of the same ID.

#### Scenario: Native use has a later neutral result
- **WHEN** a native tool use's durable assistant mirror is inside its span and a matching neutral tool result follows it
- **THEN** the native span and result retain their shared ID and are each encoded once without falling back to portable projection.

### Requirement: Streamed Sideband attempts retain evidence through body completion

A Sideband provider attempt SHALL remain within its admitted evidence and usage lifetime until its streamed response has been read, decoded, and classified. Raw provider bytes, observed native terminal, and any observed stream end SHALL be attached to that attempt before its Sideband result or failure is committed. A backend that completes at a native terminal without polling to transport EOF SHALL NOT fabricate a stream-end marker. A partial or malformed stream SHALL NOT be reported as a completed provider attempt merely because HTTP stream opening succeeded.

#### Scenario: AI shell suggestion receives a complete streamed response

- **WHEN** an AI shell-command suggestion receives a complete stream from Chat Completions, Responses, or Messages
- **THEN** the Sideband attempt evidence retains the raw response bytes and native terminal, and provider work is marked returned only after the response is collected.

#### Scenario: AI shell suggestion stream fails after opening

- **WHEN** an AI shell-command suggestion stream ends without completion evidence or contains malformed SSE after HTTP stream opening succeeds
- **THEN** the owning Timeline retains the observed response bytes and failure classification, plus any actually observed stream-end marker, before the Sideband fails; no suggestion is accepted.

### Requirement: Portable Responses history retains output message phases

来自 Responses 的 Assistant 文本 SHALL 在中性历史中保留逐条输出消息边界和 `commentary`/`final_answer` 阶段，不保留原生输出消息 ID 或状态。构造 portable Responses 请求时 SHALL 按原消息顺序及阶段投影，切换到 Chat Completions 或 Messages 时继续使用扁平正文。已有本地工具调用/结果配对和 native continuation 规则保持有效。

#### Scenario: Portable Responses phase after recovery or route switch
- **WHEN** accepted Responses output has multiple assistant messages with different phases and its native continuation is unavailable
- **THEN** durable neutral history retains their text boundaries and phases, and a subsequent Responses request emits separate assistant input messages in order with those phases.

#### Scenario: Local tool exchange follows phased messages
- **WHEN** one Responses output also contains local function calls and the corresponding results have been admitted
- **THEN** portable requests keep the complete call/result batch paired exactly once after the phased assistant messages.

#### Scenario: Legacy or transformed Assistant text
- **WHEN** an Assistant record has no boundaries or its text has been redacted/truncated so stored ranges cannot describe it
- **THEN** Responses request projection emits the existing single phase-less text message, without slicing invalid offsets or inventing a phase.

#### Scenario: Compaction replaces source messages
- **WHEN** compaction replaces a source range with a summary
- **THEN** exact phase replay is retained for un-compacted Assistant items, while the summary makes no claim to preserve discarded messages' phases.

### Requirement: Session sampler preview handoff bounds fragment backlog

A live session's sampler-to-Shell event handoff SHALL reserve capacity before forwarding each candidate text, reasoning, tool-argument, response-start, or reasoning-signature fragment. At most 4096 such fragments and approximately 64 MiB of charged fragment payload SHALL be pending across the sampler and session-event channels. The producer SHALL await capacity asynchronously so a slow session actor backpressures the provider stream, and cancellation SHALL interrupt that wait. A fragment's reservation SHALL be returned only after its translated events have been consumed from the session event FIFO. A closed downstream consumer SHALL release blocked producers. The ReplayBuffer SHALL retain at most one pending notification with a coalescing threshold no greater than the fragment byte budget. Attempt ownership, chunk indexes, control-event order, and canonical admission SHALL remain unchanged.

#### Scenario: Session consumer stalls

- **WHEN** the session actor stops consuming events while the sampler drainer continues receiving L2 fragments
- **THEN** the drainer retains credit for the unacknowledged fragment, the producer stops forwarding once its remaining credit budget fills, and the second session event channel does not accumulate additional candidate payloads without bound.

#### Scenario: Cancellation while waiting for credits

- **WHEN** an attempt is canceled after its fragment budget fills but before the actor acknowledges another fragment
- **THEN** the provider drive stops without forwarding the waiting fragment, and its attempt-discard terminal follows already-forwarded fragments in channel order.

#### Scenario: Downstream actor closes

- **WHEN** the session actor closes before acknowledging a translated fragment
- **THEN** the drainer closes the credit budget and sampler receiver, so a producer waiting for capacity exits without retaining the payload indefinitely.

#### Scenario: Drainer resumes

- **WHEN** the actor consumes the queued translated notification and its acknowledgement fence
- **THEN** the corresponding credits are returned, the producer may continue, and text/reasoning/tool fragment order and chunk indexes remain unchanged through the terminal event.

### Requirement: Oversized sampler preview events fail before enqueue

A live session sampler handoff SHALL reject an individual charged preview fragment whose payload exceeds its 64 MiB credit capacity. It SHALL fail the current attempt locally before acquiring credits or forwarding the event, rather than charging a capped cost for an oversized item. Control and terminal events SHALL retain their existing uncharged path.

#### Scenario: One fragment exceeds all credits

- **WHEN** a text, reasoning, tool-argument, response-start or signature fragment has more charged bytes than the per-session handoff capacity
- **THEN** the producer fails the attempt without enqueuing that fragment, and no downstream credit accounting is corrupted.

#### Scenario: Fragment exactly fits all credits

- **WHEN** a charged fragment exactly equals the handoff capacity
- **THEN** it can reserve all credits and proceed under the ordinary acknowledgement fence.
