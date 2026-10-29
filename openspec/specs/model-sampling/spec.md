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

A recovery pass SHALL reuse one validated durable-history load for its uncached image-description queries. It SHALL preserve the validated historical source revision, exact source Surface identity, prompt and completed-result provenance checks, including integrity validation of other Sidebands required by the parent Timeline. A matching neutral description from an earlier revision MAY be reused when its same immutable image source remains in the current branch; unrelated appends SHALL NOT invalidate it. A queried revision beyond the durable parent or a description from a later revision SHALL NOT be accepted. The new ImageProjection SHALL still bind the exact current source revision and image fingerprint. The pass SHALL NOT retain the full loaded history across auxiliary provider requests.

#### Scenario: Several groups need durable reuse

- **WHEN** a recovery pass has multiple uncached image groups
- **THEN** their durable lookups share one history load and each query receives only its own matching completed result

#### Scenario: Historical Sideband integrity fails

- **WHEN** required Sideband validation fails during batch lookup
- **THEN** no result is accepted from that failed snapshot and existing description failure or provider fallback handling applies

#### Scenario: Every group is cached

- **WHEN** all group descriptions are available in the session cache
- **THEN** the pass performs no durable-description history load

#### Scenario: A new input follows a completed transcription

- **WHEN** a new input advances the Surface revision while the same immutable image source and neutral prompt remain selected
- **THEN** recovery reuses the older completed description and creates its projection against the current exact revision without another auxiliary call

#### Scenario: Future revision or different provenance

- **WHEN** a lookup uses a revision newer than the durable parent, a candidate describes a later revision, or source identity, selected branch, prompt or completed result does not match
- **THEN** that candidate is not accepted as a reusable description

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

#### Scenario: Text-only content type rejection

- **WHEN** an image-bearing request receives HTTP 400 with the explicit claim `messages.content.type 参数非法，取值范围 ['text']` (or the same list using double quotes, optionally followed by whitespace or terminal punctuation)
- **THEN** the shared classifier treats it as an unconditional image-input rejection and the session follows the existing visual auxiliary/OCR and durable image-projection flow before rebuilding the request.

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

For explicitly unsupported image input, Grow SHALL send an available configured visual auxiliary model only the images in the image group and the fixed English default prompt: `Transcribe the attached image into a neutral, standalone textual description for a model that cannot see images. Preserve all visible text exactly where practical and in its original language, along with structure, spatial relationships, UI state, diagrams, tables, code, errors, and other concrete details. Do not translate, tailor the description to a presumed task, or infer from conversation context.` The request SHALL NOT include conversation history, tool or Skill context, or source message text. Grow SHALL retain the source Surface reference as local Sideband provenance and use the fixed prompt identity for cache and durable-result matching. Image recovery progress and outcome SHALL use one stable operation row as specified by `client-surfaces`; the row SHALL report vision, OCR, completion, partial completion and failure in English rather than emitting a recovery NOTICE. If the auxiliary model is absent or fails, Grow SHALL attempt local OCR. Successful OCR and visual descriptions SHALL preserve recognized/source language without translation and be retained and reused for marked models. Every still-unresolved image group SHALL receive a typed unsupported-model projection with the existing canonical replacement `当前模型不支持多模态，图片已经被删除`; projection failure SHALL preserve the original image evidence and settle the operation row with an English failure state. Description, OCR and removal shadows for one Surface revision SHALL commit atomically before the primary request is rebuilt. Mixed and removal-only outcomes SHALL report their counts in the same English operation row.

#### Scenario: Auxiliary description succeeds

- **WHEN** a configured visual auxiliary model produces a nonempty description
- **THEN** its provider request contains only the group's images and the fixed English prompt, with no conversation history, tools, Skills or source message text; Grow retains the description in its original language and retries the primary request after the projection ACK.

#### Scenario: Auxiliary unavailable

- **WHEN** the visual auxiliary route is unconfigured or fails
- **THEN** the same operation row changes to English OCR progress and Grow attempts local OCR; a successful result is retained in its original language without translation.

#### Scenario: Both fallbacks fail

- **WHEN** neither visual description nor local OCR can provide text for an image group after the primary model explicitly rejects images
- **THEN** Grow durably replaces that group in the current Surface with one existing canonical `当前模型不支持多模态，图片已经被删除`, removes its raw image parts and retries the primary request without those images, while the same operation row reports the outcome in English.

#### Scenario: Mixed fallback outcomes

- **WHEN** one rejected request contains groups that are described successfully and groups that remain unresolved
- **THEN** one exact-revision projection atomically attaches descriptions to successful groups and removes only unresolved groups, without repeated per-image notices, and the same operation row reports the partial outcome in English.

#### Scenario: Image fallback fails without damaging the session

- **WHEN** unsupported-image removal is acknowledged and the user later sends a text-only follow-up
- **THEN** the new request uses the repaired Surface, contains none of the removed historical images and does not repeat the same capability 400 merely because the raw Timeline evidence still exists.

#### Scenario: Image projection cannot be committed

- **WHEN** projection validation, durable write or acknowledgement fails
- **THEN** Grow does not resubmit an in-memory-only lossy request, settles the same operation row with an English failure state, and preserves the original Timeline evidence for exact retry or explicit recovery.

#### Scenario: Reuse completed description after unrelated context changes

- **WHEN** the same immutable image source is selected after unrelated messages, tool results or Skill context have changed, or after recovery
- **THEN** Grow matches the completed result using the fixed prompt identity and exact source/image provenance, reuses it without sending that contextual text to the auxiliary provider, and binds a new projection to the current exact source revision.

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

主/子 agent 模型步骤的每次真实 provider attempt SHALL 具有独立于 Goal 存在与否的归属，并向所有适用的现有消费账本结算，包括最终未被接纳的候选。结算 SHALL 按归属幂等并等待确认；成功响应接纳 SHALL NOT 再累计已结算的消费。总 token 消费用量未知时 SHALL 保留未知状态，精确预算下关闭准入。full input 与 output 已知但缓存分类缺失时 SHALL 结算精确总消费和未知分类，不因此关闭总 token 预算准入。重试 SHALL 重新检查剩余子任务输出额度。

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
- **WHEN** 已开始的 attempt 无法取得可信的完整总 token 消费且受精确 Goal 或子任务预算约束
- **THEN** 记录用量不完整并关闭后续准入，不把未知消费当零。

#### Scenario: Unknown spend without an exact budget
- **WHEN** 总 token 消费用量不完整但没有精确预算约束且其他恢复条件均满足
- **THEN** 确认记录未知状态后可继续有界恢复，已知下界不冒充完整用量。

#### Scenario: Remaining output grant shrinks
- **WHEN** 失败 attempt 的已知输出消费减少子任务剩余额度
- **THEN** 下一 attempt 的输出上限不超过新余额，余额耗尽则不派发。

#### Scenario: Known totals with missing cache details
- **WHEN** attempt 的 full input 与 output 可确认，但 cache read 或 write 没有报告
- **THEN** 所有适用账本结算总消费各一次并保留分类未知，精确 token 预算按该总量检查，完整响应不因分类缺失重新生成。

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

#### Scenario: Model skill metadata separates completed results
- **WHEN** Model SkillLoad 紧邻其匹配结果且后面还有同批真实结果，历史重建或 native prefix 重置
- **THEN** 请求保留全部真实调用和结果各一次，元数据不截断批次、不产生孤立 output，原 native 内容不变。

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

#### Scenario: Runtime-only Chat history requires reasoning replay
- **WHEN** Chat 路由明确要求 reasoning_content，而当前历史只有会生成 assistant 的非空 AgentMessage 或用户 SkillLoad
- **THEN** 可确认一次启用，portable 与新增 live Runtime assistant 均编码空 reasoning_content，不借用无关 reasoning，不改变既有恢复额度；空收件批次或 Model-only 技能元数据不构成启用依据。

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

有来源的 runtime agent-message context item SHALL 在 Chat/Messages 请求中投影为完整、可配对的专用收件工具调用/结果，使用从 receipt 确定的稳定身份；Responses SHALL 使用明确标记来源的独立 user 上下文，携带完整收件 JSON 并声明不是直接用户指令或人工授权，不生成模拟模型调用或 reasoning。该投影 SHALL NOT 冒充模型主动执行或触发工具 dispatch，不得追加孤立 ToolResult 或篡改其他真实工具的结果。canonical Surface 坐标 SHALL 不因 provider 一对多展开改变。

#### Scenario: Receiving a message during a tool batch
- **WHEN** 消息到达而现有真实工具批次尚未闭合
- **THEN** 消息在安全边界加入请求，完整收件调用/结果对不插断原工具配对。

#### Scenario: Provider switch and portable history
- **WHEN** 含已消费 agent 消息的请求切换现有 provider adapter 或供 Sideband 冻结
- **THEN** 完整正文、来源和稳定身份保留，portable projection 不把它作为孤立结果丢弃，也不执行历史调用。

#### Scenario: Compaction and source permissions
- **WHEN** 对含 runtime agent message 的 Surface 进行 compaction 或权限判定
- **THEN** input_ref 仍指向 canonical 事实，消息不获得 DirectUser/Interjection 权限证据；一对 provider item 不生成两份消费坐标。

#### Scenario: Responses thinking receives a Runtime message
- **WHEN** Responses thinking 路由收到有来源的 AgentMessage
- **THEN** 正文及 receipt/source/target/reply 身份完整出现一次，不生成 function_call/output 或伪造思考，canonical 权限和消费坐标不变。

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

### Requirement: Image budget choices remain stable between reclamation boundaries

在同一存活会话、相同请求投影域内，采样 SHALL 对已因请求图片预算替换的旧图片沿用相同替代内容，并基于应用既有选择后的请求大小判断是否再次回收。纯追加后未达到高水位时 SHALL NOT 因重新读取原图而推进旧图片淘汰边界。再次达到高水位时 SHALL 按时间顺序回收至低水位或可回收图片耗尽；原始 Timeline 图片证据 SHALL 保留，最终协议请求仍受既有字节上限约束。

#### Scenario: Append after the first reclamation
- **WHEN** 一次请求已批量淘汰旧图片，后续请求只追加新消息或图片，应用既有淘汰选择后仍低于高水位
- **THEN** 此前图片与 placeholder 的选择保持不变，不因预算算法额外改写历史、重置 native continuation epoch 或改变其派生 cache key。

#### Scenario: Reach the next reclamation boundary
- **WHEN** 应用既有淘汰选择后的请求再次达到高水位
- **THEN** 按 oldest-first 扩大淘汰范围，后续未达高水位的请求沿用新范围；真实历史投影变化继续经过既有 native 失效判断。

#### Scenario: Multiple image parts in a tool result
- **WHEN** 旧 User 或 ToolResult 包含多个图片及文本 part，并重复构造三种 backend 的请求
- **THEN** 被选择的图片持续位于原因果位置并使用相同替代文本，其他附件和工具关联保持有效，Timeline 原图不被请求预算修改。

#### Scenario: Non-image content exceeds the limit
- **WHEN** 已回收可回收图片但最终编码后的协议请求仍超过现有请求字节上限
- **THEN** 请求按既有大小失败路径终止，不通过恢复原图、绕过上限或无限预算重试发送。

证据入口：`crates/codegen/chat-state/src/actor/request_builder.rs` — `build_conversation_request`、`compact_images_to_byte_budget`；`crates/codegen/sampler/src/client.rs` — `check_and_record_request`。新增跨 build 回归属于本 change 待实现内容。

### Requirement: Image budget choices follow their request projection lifetime

图片预算选择 SHALL 仅适用于其源 Surface 身份和图片呈现方式。源历史或投影域被替换时 SHALL 基于新 Surface 重建选择；单纯 native continuation reset SHALL NOT 清除仍然有效的预算选择。fork 和冷恢复 SHALL 独立建立临时选择，不从历史请求证据恢复 native 状态，也不复活当前 Surface 已明确移除的图片。

#### Scenario: Native protocol recovery without image changes
- **WHEN** 当前路由撤销 native continuation，但源 Surface、图片呈现和预算没有变化
- **THEN** 下一次请求继续沿用既有图片替代选择，撤销的 native 数据不因此恢复。

#### Scenario: Surface or presentation changes
- **WHEN** rewind、compaction、旧 Surface 替换、route 替换、goal 过滤域或图片描述呈现域改变
- **THEN** 旧选择不应用到不同身份或内容的 part，按当前投影建立新选择；恢复和 native 规则继续独立执行。

#### Scenario: Fork, cold restore and resident reconnect
- **WHEN** 创建 fork 或冷恢复 actor，或客户端重新连接原存活 actor
- **THEN** 前两者从各自当前 Surface 建立选择，后者保留原选择；均不得修改 immutable 图片证据。

证据入口：`crates/codegen/chat-state/src/actor/state.rs` — `ContinuationLane`；`actor/mod.rs` 的 route/config 控制入口；`actor/mutations.rs` 的 Surface 替换入口。

### Requirement: Cache usage preserves field availability across protocols

采样归一化 SHALL 分别保留缓存读取和写入字段的可用性，区分未报告与明确报告 0。别名只在主字段缺失且含义已验证时采用；冲突或越界的缓存分类 SHALL 标为不可用并保留诊断事实，不覆盖独立可信的总输入输出。full input SHALL 表示全部输入，不能把仅含 uncached input 的值当作完整输入。

#### Scenario: Missing and explicit zero
- **WHEN** 两个 Chat、Responses 或 Messages 响应分别省略 cache read 与明确给出 read=0，且适用协议没有缺失等价于零的可靠约定
- **THEN** 前者保留未知，后者保留已知零；后续归一化和结算仍能区分二者。

#### Scenario: Independent cache read and write
- **WHEN** response 只报告 read 或只报告 write，包括 Responses 的非零 cache_write_tokens
- **THEN** 已报告桶保留精确数值，未报告桶保持未知，write 不计入 cache hit。

#### Scenario: Alias precedence and conflict
- **WHEN** 主缓存字段明确为 0、兼容别名也存在，或两者报告不同数值
- **THEN** 不把 0 当缺失；一致时只计一次，不一致时将该桶标为未知并保留冲突证据。

#### Scenario: Messages start and delta
- **WHEN** Messages 的 delta 省略缓存字段或明确给出 0
- **THEN** 省略沿用 start 中该字段的数值或未知状态，明确 0 覆盖 start；从未报告的桶不被默认成 0。

#### Scenario: Full input cannot be reconstructed
- **WHEN** 协议只报告 uncached input，缺少构成 full input 所必需的缓存桶，且没有独立总量或该协议缺失等于零的可靠约定
- **THEN** 总输入保持不完整，沿现有未知消费结算与预算路径处理，不伪造完整输入，也不因此重采样已合法完成的响应。

#### Scenario: Cache counts exceed full input
- **WHEN** 独立总量可信但缓存读写违反适用协议的输入桶约束
- **THEN** 受影响缓存分类不参与命中统计，总输入输出仍按其可信值结算，原始响应证据保留。

证据入口：`crates/codegen/sampling-types/src/{types,messages,conversation}.rs`；`crates/codegen/sampler/src/stream/{chat_completions,responses,messages}.rs`。字段 presence 和冲突 fixtures 为本 change 待实现验证。

### Requirement: Cancellation preserves confirmed attempt usage

Provider attempt 在取消或逻辑期限结束前，若已按当前协议与归一化规则确认完整总消费，采样 SHALL 保留该 usage、缓存字段可用性及已报告费用，并以原 attempt 身份向所有适用账本结算各一次；取消可丢弃候选正文，但 SHALL NOT 仅因未发布响应终态而丢弃已确认的消费。没有可信完整总量时 SHALL 沿既有未知结算规则处理，保留费用与精确预算限制。取消 SHALL NOT 为收集尚未收到的 usage 延长 provider 工作、启动重试或新请求；已准入的 evidence 与结算确认仍遵循现有完成屏障。

#### Scenario: Chat usage observed before a pending stream tail

- **WHEN** Chat 已有 finish reason 并解析确认完整 usage，随后结束帧或 EOF 持续 pending，此时用户取消或 steer
- **THEN** 该 usage 按原 attempt 身份持久结算一次，缓存读写可用性与已报告费用保留；请求按取消结束，不等待剩余尾部、不接纳候选、不执行其工具、不重试。

#### Scenario: Final Chat chunk contains preview and usage

- **WHEN** 同一 Chat chunk 包含最后一段可见输出、finish reason 和完整 usage，用户在该段预览交出后立即取消
- **THEN** 解析器在交出预览前已确认并暂存完整用量；该 attempt 沿取消路径结算 Known，不因解析器在预览 yield 处被丢弃而改记 Unknown。

#### Scenario: Logical deadline during the same tail wait

- **WHEN** 完整 usage 已确认但流尾仍 pending，逻辑采样期限触发取消并转换为期限失败
- **THEN** 失败保留原 attempt 的已确认用量，完成既有 evidence 与账本 ACK 后结束，不把已知消费降为未知，不重新起计时期限。

#### Scenario: Cancellation before trustworthy usage

- **WHEN** provider 已开始工作，但取消时只收到内容或不满足当前协议完整性条件的 usage 中间值
- **THEN** 该 attempt 以未知总消费结算，不用估算 token、缓存命中或零值代替缺失字段；精确预算仍按未知消费关闭后续准入。

#### Scenario: No provider attempt admitted

- **WHEN** 取消发生在任何 provider attempt 获得准入之前
- **THEN** 不因本次取消新增 provider 消费或未知消费结算，沿既有取消生命周期结束。

#### Scenario: Buffered terminal competes with cancellation

- **WHEN** 完整终态与取消同时可见，或取消发生在已确认用量的持久结算 ACK 等待期间
- **THEN** 该 attempt 的已确认用量只结算一次，ACK 屏障不被跳过，不因取消再写一份 unknown，也不因重复终态二次累计。

#### Scenario: Attempt isolation

- **WHEN** 已确认 usage 的 attempt 结束后，同一逻辑请求的另一 attempt 或另一请求在没有完整 usage 时被取消
- **THEN** 后者仍按自己的证据结算为未知，不能借用先前 attempt 的用量或费用。

#### Scenario: Protocol completeness remains authoritative

- **WHEN** Messages 只有 terminal delta、缺少当前完整 usage 所需的 message_stop 或 full input 构成，或者 Responses 尚无可信 terminal usage
- **THEN** 取消继续保留未知总消费；本要求不把中间累计值提升为精确账单，也不把已知账单提升为候选接纳许可。

证据入口：`crates/codegen/sampler/src/stream/chat_completions.rs::stream_chat_completions`、`crates/codegen/sampler/src/actor/request_task.rs::{run_one_attempt,drive_l2}`、同文件 `drive_l2_buffered_terminal_outranks_simultaneous_cancel_and_preserves_usage`；既有结算入口为 `crates/codegen/shell/src/session/actor/turn/sampling.rs::sampling_usage_sink`。Chat 尾部窗口新增场景待实施验证。

### Requirement: Model API backend overrides its provider default

Grow SHALL select a configured model's API backend from its explicit `api_backend` when present, otherwise from its provider's `api_backend` default, otherwise from `chat_completions`. The resolved backend SHALL be carried into that model's sampling configuration and used to choose its request protocol. Models within the same provider MAY use different backends.

#### Scenario: Model inherits provider backend

- **WHEN** a provider sets `api_backend = "responses"` and one of its models omits `api_backend`
- **THEN** that model's sampling configuration uses Responses.

#### Scenario: Model overrides provider backend

- **WHEN** a provider sets `api_backend = "responses"` and another model explicitly sets `api_backend = "messages"` or `"chat_completions"`
- **THEN** that model's sampling configuration uses its explicit backend, independently of the first model.

#### Scenario: Neither level specifies a backend

- **WHEN** a provider and its model both omit `api_backend`
- **THEN** that model's sampling configuration uses Chat Completions.

### Requirement: Local finalization failure preserves admitted attempt settlement

本地子任务收尾失败或 caller 超时 SHALL 保留已经准入的 attempt evidence、用量结算与关闭责任，继续遵循既有 cancellation/shutdown 屏障。此类失败 SHALL NOT 单独触发 provider 重采样、工具重执行或提前关闭 child Timeline。强制关闭期限到期 SHALL 报告关闭未完成，不能声明已成功结算。

#### Scenario: Provider completed before result storage failed
- **WHEN** provider 已正常返回，随后本地结果写入或父回执提交失败
- **THEN** 恢复只核对本地事实与交付，不重新请求模型生成相同结果。

#### Scenario: Cancellation arrives during usage settlement
- **WHEN** 子任务取消时已准入 attempt 的证据或用量 ACK 尚未确认
- **THEN** 系统保持可观察的 settling 状态，不以 caller 超时为由提交 child terminal；既有关闭期限到期则报告关闭失败并保留未确认边界。

#### Scenario: Tool side effects cannot be confirmed
- **WHEN** 工具可能已产生外部副作用，而完成记录或响应不明
- **THEN** 收尾恢复不自动重执行工具，也不将缺失结果当作该工具从未执行。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/handle_request.rs`、`crates/codegen/shell/src/session/actor/teardown.rs`、`crates/codegen/sampler/src/actor/mod.rs`；既有 sampler/child cancellation 结算契约继续适用。

### Requirement: Chat usage accepts null optional token details

Chat Completions SHALL decode optional usage detail counts as unknown when omitted or explicitly `null`. Explicit zero and positive integer values SHALL remain available as reported values. This applies to prompt `audio_tokens` and completion `reasoning_tokens`, `audio_tokens`, `accepted_prediction_tokens`, and `rejected_prediction_tokens`, for both streaming and non-streaming responses. Nullable details SHALL NOT invalidate otherwise complete usage or response facts. Trusted prompt, completion and total counts, cache-token handling, and existing usage settlement behavior SHALL remain unchanged; unknown reasoning counts SHALL continue through the existing aggregation boundary as zero.

The decoder SHALL continue to reject negative, string, floating-point, and out-of-range token counts, and SHALL reject `null` for required usage totals. It SHALL NOT skip malformed chunks or change retry behavior.

#### Scenario: Optional detail omitted or null

- **WHEN** a streaming or non-streaming Chat response omits an optional detail count or supplies it as `null`
- **THEN** decoding succeeds and that detail remains unknown, while valid total usage and response facts remain available

#### Scenario: Optional detail reports zero or a positive count

- **WHEN** a streaming or non-streaming Chat response supplies an optional detail count as zero or a positive integer
- **THEN** decoding preserves that reported value, including the distinction between zero and unknown

#### Scenario: Invalid optional count or null required total

- **WHEN** a Chat usage detail is negative, a string, a floating-point value, out of range, or a required total is `null`
- **THEN** decoding rejects the malformed usage rather than coercing or skipping it

#### Scenario: Tool-call completion followed by nullable usage tail

- **WHEN** an HTTP Chat SSE response reports `finish_reason: tool_calls` and its usage tail contains nullable optional details with valid required totals
- **THEN** the response completes and trusted usage is settled through the existing accounting path without changing retry behavior

### Requirement: Negative image capability follows the resolved transport

Grow SHALL scope a confirmed negative image-input capability to the resolved transport identity consisting of the actual provider-facing model, API backend, endpoint and query route, as represented by `ModelImageInputKey`. Catalog aliases SHALL NOT determine whether a transport has previously rejected images. A different alias resolving to the same transport SHALL share its negative capability; any change to model, backend, endpoint or query route SHALL produce an independent capability state. Primary sampling, configured visual auxiliary sampling and compaction SHALL use the same identity rule. A configured auxiliary resolving to the same transport already known to reject the primary image request SHALL NOT be invoked recursively. User-visible model labels SHALL continue to use their catalog identity.

This capability state SHALL only affect whether the exact resolved transport is treated as previously known to reject images. An unmarked transport SHALL receive original image content on its first request. After an explicit rejection, the existing visual auxiliary, local OCR, durable `ImageProjection` acknowledgement and request reconstruction flow SHALL remain in force. Descriptions keyed to the same source and image group MAY be reused after the auxiliary transport changes.

#### Scenario: Same alias resolves to a changed transport

- **WHEN** a catalog alias previously received an explicit image rejection but now resolves to a different model, backend, endpoint or query route
- **THEN** the new transport has no inherited negative image capability and its first request may include the retained original image

#### Scenario: Different aliases resolve to the same rejected transport

- **WHEN** one alias receives an explicit image rejection and another alias resolves to the same model, backend, endpoint and query route
- **THEN** the second alias observes the same negative capability and uses the existing marked-model image representation without repeating the rejected image request

#### Scenario: Auxiliary alias resolves to the rejected primary transport

- **WHEN** the configured auxiliary alias resolves to the exact transport that rejected the primary image request
- **THEN** Grow skips the auxiliary call to that runtime and continues through the existing OCR/removal projection flow without recursive retry

#### Scenario: Auxiliary alias resolves to a different transport

- **WHEN** a configured auxiliary alias retains its name but resolves to a transport whose model, backend, endpoint or query route differs from the rejected primary transport
- **THEN** Grow treats it as an independent runtime and may invoke it through the newly resolved route

#### Scenario: Primary or auxiliary runtime rejects image input

- **WHEN** a primary or configured auxiliary transport explicitly rejects an image-bearing request
- **THEN** Grow records the rejection under that request's resolved transport identity, and later lookup by any alias uses the same identity rule

#### Scenario: Compaction checks a model's image capability

- **WHEN** compaction evaluates whether an image-bearing request can use its resolved primary transport
- **THEN** it consults the same transport-bound negative capability state used by normal sampling

#### Scenario: A pending image compaction outlives its transport route

- **WHEN** the model, backend, endpoint or query route changes while an image-bearing compaction request is pending
- **THEN** its old rejection does not mark or recover the new transport; an alias-only change to the same transport does not invalidate its identity

### Requirement: Image fallback respects cancellation and cold recovery

Image fallback SHALL stop new auxiliary/primary provider requests and new OCR processes when its owning foreground turn is cancelled. Completed neutral image descriptions with valid durable source provenance SHALL be reused by a fresh actor after recovery, including a crash after the completed description but before its image projection. Cancellation SHALL NOT be treated as ordinary auxiliary failure that authorizes later fallback work or a lossy projection. Existing OCR output validation, original media retention and projection acknowledgement rules SHALL remain in force.

#### Scenario: Auxiliary preparation is cancelled

- **WHEN** the owning turn is cancelled before its auxiliary provider request crosses admission
- **THEN** no auxiliary HTTP request, OCR process, image projection or primary resubmission is started for that cancelled recovery

#### Scenario: First image group is cancelled during auxiliary work

- **WHEN** a turn with multiple unresolved image groups is cancelled while its first auxiliary request is active
- **THEN** that work is cancelled and no later group's auxiliary request, OCR fallback or primary resubmission begins

#### Scenario: OCR produces valid text

- **WHEN** the configured auxiliary fails and local OCR produces nonempty valid text
- **THEN** a durable OCR projection retains the original image and its text, and the marked primary retries with text only after acknowledgement

#### Scenario: OCR cannot produce text

- **WHEN** the configured auxiliary fails and local OCR is empty or fails
- **THEN** the existing durable unsupported-image removal is acknowledged before a text-only primary retry, while immutable original evidence remains

#### Scenario: Recover after description completion before projection

- **WHEN** a fresh actor restores a validated Timeline and completed image-description Sideband after a crash before ImageProjection
- **THEN** it reuses the matching neutral description, installs a valid projection and sends a text-only primary request without another auxiliary request

#### Scenario: Recover a completed description without a current auxiliary route

- **WHEN** a valid completed neutral description exists but the auxiliary route is absent or cannot be used after recovery
- **THEN** its reuse is evaluated independently of that route and does not fall through to OCR or removal merely because new auxiliary work is unavailable

#### Scenario: Recover an already described image

- **WHEN** a fresh actor restores an image with an acknowledged description or OCR projection and its marked primary continues
- **THEN** the existing description is reused without auxiliary work and the primary request retains the textual representation

### Requirement: Suspension settles the current attempt before saving continuation

快速挂起 SHALL 停止新采样准入并中止活跃 provider 请求，保留可得 evidence 与已知或 incomplete 用量结算。只有结算已确认或在合法恢复所有权下核对完毕，才能确认对应挂起位置；取消或超时 SHALL NOT 代替结算 ACK。挂起中断不代表 child 任务取消，已经接纳的 assistant response SHALL 保持 canonical。未接纳步骤的重新采样 SHALL 继续检查 typed attempt 事实、输出可撤销性、外部副作用安全、结算、当前 owner/预算准入及剩余恢复额度；挂起与 resume SHALL NOT 重置原逻辑采样的总 attempt 上限或绝对期限。

#### Scenario: Sampling is interrupted before response admission
- **WHEN** 活跃流仅有尚未接纳的 candidate 时发生挂起
- **THEN** 保存 attempt 中断与用量证据，不把部分输出变成成功回复；恢复核对全部重新采样条件，仅在满足时于原逻辑步骤发起新 attempt，不重放历史工具副作用；条件不满足时明确报告受阻或沿既有失败路径处理。

#### Scenario: Provider-side effects remain uncertain after suspension
- **WHEN** 未接纳的 provider 请求可能执行有副作用的托管工具，且无法证明重放安全
- **THEN** 即使 Grow 本地工具尚未执行，也不自动重采样；保留原 attempt 的未知副作用事实，等待核对或明确处置。

#### Scenario: Retry allowance expires while suspended
- **WHEN** 恢复时原逻辑采样总 attempt 额度已耗尽或绝对期限已到
- **THEN** 不以新运行实例或重新提交为由重置额度/期限，不再派发该步骤的自动重试，并沿既有额度或期限失败路径处理。

#### Scenario: Response admission already committed
- **WHEN** response 已接纳但其工具尚未全部执行时发生挂起
- **THEN** 恢复复用该 canonical response 和调用身份，先核对或补齐既有展示提交，再续接剩余工具，不重新请求同一已接纳回复。

#### Scenario: Attempt settlement is blocked
- **WHEN** evidence 或用量确认超过挂起等待期限
- **THEN** 挂起返回未确认，保存已知身份，不以丢弃 request task、零用量或重复结算制造成功保存边界。

目标入口：`crates/codegen/sampler/src/actor/mod.rs`、`crates/codegen/shell/src/session/actor/teardown.rs::shutdown_sampler` 和 response admission 路径。

### Requirement: Runtime skill results have legal provider projections
Runtime 来源的技能结果 SHALL 在各受支持请求协议中形成合法且完整的上下文表示，不含孤立工具结果、不拆开已有工具批次、不冲突 call identity。协议投影 SHALL 不改变内部真实来源，不生成虚假的采样请求、响应、用量或可调度模型调用。原始用户消息 SHALL 不包含自动追加的技能正文。

#### Scenario: Portable provider request
- **WHEN** Chat Completions、Responses 或 Messages 请求包含用户触发的技能加载结果
- **THEN** 请求包含完整加载内容；Responses 使用独立 user 上下文表达 Runtime 加载结果，保留结构化来源和加载身份，不生成模拟 function_call/output 或 reasoning；Chat Completions 与 Messages 保留协议配对。没有孤立或重复结果，Timeline 仍记录 Runtime 来源。

#### Scenario: Existing model tool batch
- **WHEN** 一个模型批次存在多个尚在结算的工具调用，同时收到技能插话
- **THEN** 加载投影放在批次安全边界，不把 Runtime 工具往返插进尚未完成的调用结果配对之间。

#### Scenario: Native continuation
- **WHEN** 已有 native continuation 的下一次请求需要加入 Runtime 技能结果
- **THEN** 使用可安全表达该结果的增量路径或现有 portable 重建路径；不能表达时明确终止该次执行并结算尚未消费的加载贡献，不遗漏正文继续请求。

证据入口：`crates/codegen/sampling-types/src/conversation.rs` 的 AgentMessage 配对展开、工具 ID 去重和各 backend 转换；该模式是参考，本条新增 Skill 事实投影。

#### Scenario: Thinking route receives a user skill without model reasoning
- **WHEN** Responses thinking 路由首次或在 portable/native 历史后收到用户加载的技能，且该 Runtime 事实没有模型思考
- **THEN** 完整技能结果作为独立 user 消息出现一次，原始用户输入不改写，不依赖先触发 400、学习 reasoning 回放或用户再发继续；既有 native reasoning 与真实模型工具调用保持不变。

#### Scenario: Model-triggered skill load
- **WHEN** Responses 请求包含模型真实调用产生的技能加载事实及匹配工具结果
- **THEN** 仅保留真实调用/结果，不追加用户技能消息或重复技能正文。

### Requirement: Loaded skill content must fit its dependent request
Loaded SHALL 表示完整准备正文已获上下文发布确认，不等于后续采样必然成功。运行时 SHALL 在权威发布点及首次关联请求构造时复核当前 Surface revision、模型 / 路由与完整请求预算，覆盖用户输入、附件、技能正文、协议包装、工具定义及输出预留。首次关联步骤获得已接纳响应前，恢复重试 SHALL 保留全文；不得以静默截断、blob 地址或延后自主读取代替交付。历史 Loaded SHALL 不等于永久正文可用。

#### Scenario: Expanded content exceeds available budget
- **WHEN** 有界准备后的必要正文与完整请求无法被当前上下文预算容纳
- **THEN** 明确返回预算失败，不宣称 Loaded 或执行依赖输入。

#### Scenario: Large original user prompt
- **WHEN** 原始用户输入触发既有长提示存储策略，同时具有技能加载结果
- **THEN** 用户输入保持该策略，独立技能结果仍满足完整交付约束，不被旧 skill inline budget 静默截断。

#### Scenario: Later compaction
- **WHEN** 首次关联步骤已获得包含完整加载内容的请求所对应的已接纳响应，历史随后被正常压缩
- **THEN** 保留当时的加载事实语义，不据此承诺完整正文仍在当前上下文；新的显式加载仍可提交新的正文结果。

#### Scenario: Model or route changes before first response
- **WHEN** 正文已发布，首次关联步骤尚未获得已接纳响应，模型缩窗或路由变化使其无法完整投影
- **THEN** 重新核对预算和投影；无法满足时明确终止该次执行，保留 Loaded 历史并撤下用户输入及正文组，不以截断内容采样，也不让残留组进入后续独立请求。

#### Scenario: Model tool result cannot be delivered
- **WHEN** 模型自主加载的已发布结果在首次消费前无法继续交付
- **THEN** 用同 call identity 的简短交付错误替换当前 Surface 中的结果，保持真实工具配对及原始历史，不留下孤立调用；替换无法确认时停止采样。

#### Scenario: Retry before an accepted response
- **WHEN** 首次含全文请求失败后恢复重试，尚无已接纳模型响应
- **THEN** 全文保护继续有效，不因第一次请求已发出而解除；终止本次执行时先结算未消费内容，再允许后续独立输入。

#### Scenario: Existing image projection updates a pending group
- **WHEN** 首次已接纳响应前，用户组的附件按既有图片投影规则更新 Surface identity
- **THEN** 组关联沿来源关系跟随当前项，继续保护技能正文全文；若随后终止，撤下当前用户项及其技能结果，不因原 ID 被替换而留下失败输入。

证据入口：`crates/codegen/shell/src/session/actor/prompt_build.rs` 的 LARGE_PROMPT_THRESHOLD / SKILL_INLINE_BUDGET 与现有 context admission/compaction 路径。本条不更改文件读取的 1 MiB 上限。

### Requirement: Manual continuation survives exhausted provider generation

After a provider generation failure exhausts automatic recovery, a live session SHALL settle the failed Step and Turn and release foreground ownership through the ordinary completion path. A later admitted user prompt SHALL start a new Turn in that same resident session with a fresh logical sampling budget, without restarting Grow or replacing the actor. Valid prior history and all durable spending SHALL remain authoritative; rejected candidate calls and native continuation SHALL NOT enter the next request.

#### Scenario: Three malformed completed responses followed by manual continuation
- **WHEN** retractable delivery and the existing budgets permit three attempts, Chat Completions, Responses or Messages returns three complete identity-valid candidates containing invalid tool-argument JSON, automatic recovery stops, and the provider later returns valid output after the user submits `继续`
- **THEN** the same resident actor completes the new Turn, no rejected tool call executes, and all four actual attempts are accounted once with distinct request-local recovery ownership.

#### Scenario: User input arrives while the failed Turn settles
- **WHEN** a user prompt is admitted while the failed Turn is committing its terminal or releasing foreground ownership
- **THEN** normal input ordering promotes it only after the predecessor's durable terminal, without a lost input, duplicate terminal or manual Idle reset.

#### Scenario: Protocol JSON is malformed
- **WHEN** a provider response fails deterministic protocol JSON parsing and the user sends a later prompt after the provider recovers
- **THEN** the same session can admit the new Turn, while the malformed protocol response retains its existing automatic-retry classification.

### Requirement: Sampling failure preserves settled child output grants

A child output grant SHALL be charged by confirmed attempt usage. A sampling failure alone SHALL NOT mark accurately settled spending incomplete or exhaust an otherwise available grant. Unknown consumption, failed usage settlement and actual exhaustion SHALL continue to close provider admission. A new user Turn SHALL NOT replenish the grant or erase prior spending.

#### Scenario: Known malformed generation leaves output capacity
- **WHEN** a budgeted child exhausts malformed-generation retries, every attempt has confirmed output usage, and grant capacity remains
- **THEN** its usage remains complete and exact, and any subsequently admitted work is limited to the remaining grant.

#### Scenario: Known output exhausts the grant
- **WHEN** rejected attempts consume the child's remaining output grant
- **THEN** no further provider attempt is admitted, including after new user input, and consumption is not falsely classified as unknown merely because sampling failed.

#### Scenario: Failed attempt has unknown usage
- **WHEN** a child's failed attempt has no trustworthy output usage or its usage cannot be confirmed
- **THEN** its grant remains incomplete and closed, and manual continuation cannot bypass the gate.

#### Scenario: Sampler terminal cannot be confirmed
- **WHEN** the ordered sampler terminal does not reach the session's stream-drained acknowledgement barrier
- **THEN** the child grant closes conservatively as incomplete, even if earlier attempt usage was known, and new user input does not reopen it or overwrite existing usage ledger facts.

### Requirement: Suspension rejects new auxiliary command work while draining admitted owners

Acceptance of a valid physical session suspension SHALL close top-level auxiliary activity admission before asynchronous suspension persistence. A mailbox command that cannot obtain its existing activity permit SHALL terminate through the normal closed-session response path without panicking, admitting provider work or performing that command's asynchronous mutations. Best-effort command/prompt suggestions SHALL return no suggestion. The mailbox SHALL continue servicing existing settlement, runtime observation and suspension/restore control messages. Already admitted owners SHALL retain their existing cancellation and durable settlement obligations; deadline expiry SHALL NOT reopen admission or authorize a checkpoint before their drain.

#### Scenario: Late autocomplete after suspension admission
- **WHEN** AISuggest or SuggestPrompt is delivered after a valid suspension request has closed auxiliary admission
- **THEN** it returns no suggestion, starts no provider attempt, and does not panic or prevent lifecycle and observation commands from being processed.

#### Scenario: Suspension request persistence has not acknowledged
- **WHEN** the valid suspension owner is waiting for its initial durable request acknowledgement and a finite auxiliary command arrives
- **THEN** that new work is rejected before provider or command effects while the original suspension owner retains its identity and outstanding persistence.

#### Scenario: Existing owner outlives the caller deadline
- **WHEN** an already admitted auxiliary owner has not settled by the shared caller deadline
- **THEN** the caller receives an unconfirmed result, new finite work remains blocked, no checkpoint is acknowledged early, and the existing owner can later settle through the still-live mailbox. If its drain finishes after the deadline, the Session returns unconfirmed before dispatching new checkpoint writes with that expired deadline; a new request can retry the same operation with a fresh deadline without resetting the healthy writer.

#### Scenario: Finite command arrived before the gate closed
- **WHEN** a finite command obtained its permit before suspension closed admission
- **THEN** its existing cancellation and settlement are drained before the checkpoint; suspension neither discards its accounting nor reclassifies it as a newly admitted request.

#### Scenario: Settlements remain available
- **WHEN** a suspended or suspending actor receives usage facts, checkpoint queries or physical retirement for the same suspension operation
- **THEN** the existing lifecycle path remains usable without reopening auxiliary admission or recreating interrupted work.

#### Scenario: Request is already expired at admission
- **WHEN** a queued request would start a new node suspension worker but has already exhausted its caller deadline before admission
- **THEN** it returns unconfirmed without spawning a new persistence owner or poisoning the healthy Timeline writer; a later request may retry with a fresh deadline.

### Requirement: Tool runtime failures stop dependent sampling

A failure returned by the tool execution pipeline SHALL stop the admitted model turn and completion recovery before any subsequent provider request. Ordinary tool error results and permission outcomes SHALL retain their existing behavior. The existing turn terminal owner SHALL settle unfinished Tool and Step scopes in causal order, without claiming that an unconfirmed execution succeeded. Fatal diagnostics SHALL retain the original pipeline error, including when terminal persistence also fails.

#### Scenario: Tool admission fails
- **WHEN** a model response is admitted but its tool start or runtime preflight cannot be confirmed
- **THEN** no dependent model request is sent, and the original admission failure is retained rather than replaced by a later Step boundary error.

#### Scenario: Tool result or terminal acknowledgement fails
- **WHEN** tool execution has returned but result publication or Tool terminal acknowledgement fails
- **THEN** dependent sampling and completion recovery stop; unresolved scopes are settled only through confirmed existing terminal rules, or remain explicitly unconfirmed if persistence is unavailable.

#### Scenario: Cold recovery after a failed tool boundary
- **WHEN** a session is reopened from the durable prefix of a failed tool boundary
- **THEN** the prefix remains valid, no successful tool completion is fabricated, and any confirmed fatal Turn terminal retains its original failure evidence.

#### Scenario: Turn terminal fails after a tool runtime error
- **WHEN** terminal persistence fails while settling a tool pipeline failure
- **THEN** the returned failure reports the unconfirmed terminal and also preserves the original tool error.

### Requirement: Internal image projection status is not assistant speech
图片工具调用及关联 carrier 的投影 SHALL 保留原有源信息清理、原始 Timeline 证据和完整 tool call/result 关联，但 SHALL NOT 用技术占位话语代替 assistant 正文。模型请求与派生 child 背景 SHALL 排除整个 assistant 正文精确等于 `[Image-associated assistant text projected to durable text.]` 或 `[Image-associated response carrier projected to durable text.]` 的内部话语。空的无调用 assistant 槽 SHALL NOT 形成空协议消息；相同文本作为 User、ToolResult 或正常解释的一部分 SHALL 保留。原始 provider/native 证据 SHALL NOT 被改写。

#### Scenario: Successful or lossy image projection
- **WHEN** description、OCR 或 unsupported-model shadow 处理图片工具调用及其 carrier
- **THEN** 后续请求保留有效调用/result 配对与适用的描述或移除结果，但不产生内部 assistant 占位话语，live 与 cold replay 保持一致。

#### Scenario: Saved assistant placeholder is reused
- **WHEN** 历史中一条 assistant 正文精确等于两个内部保留值之一
- **THEN** 派生请求与 child 背景排除该正文，保留调用及结果；若它属于当前 native span，则用既有 portable fallback 避免原生载体重新带入。

#### Scenario: Marker appears in real content
- **WHEN** 用户/工具提供同样文字，或 assistant 在正常解释中包含该文字
- **THEN** 内容按原有语义保留，不执行通用子串过滤或修改原始持久记录。

### Requirement: Side questions reuse durable image fallback
/btw SHALL be a read-only consumer of the primary session's image projection and SHALL use its prepared primary LLM route. Before freezing Sideband context it SHALL wait for any in-progress primary image conversion, including its durable ImageProjection acknowledgement. It SHALL select every available image description from that acknowledged Surface on the request copy, independently of the negative-capability cache. It SHALL NOT start vision/OCR/conversion work, write negative image capability or retry an image rejection by mutating the primary Surface. The question and answer SHALL remain Sideband-only. Waiting SHALL observe session Sideband cancellation and SHALL NOT hold the image projection gate during its provider request or overload retries. An unsupported transport with unresolved images SHALL fail without issuing an image-bearing request; unconverted media for an unknown transport retains existing optimistic semantics.

#### Scenario: Known text-only route
- **WHEN** main context retains original images paired with acknowledged descriptions and /btw is requested
- **THEN** its frozen portable context uses the existing descriptions without raw media or another auxiliary request, including when negative capability is absent

#### Scenario: Primary conversion has not finished
- **WHEN** main vision/OCR recovery or its ImageProjection acknowledgement is pending and /btw is requested
- **THEN** /btw waits without starting a provider or conversion request, then freezes and uses the acknowledged text branch once main conversion completes

#### Scenario: Unrelated failure or failed projection
- **WHEN** main conversion cannot commit a usable text branch or the waiting SideQuestion is cancelled
- **THEN** /btw returns failure without independently converting images or emitting an unacknowledged textual provider request

#### Scenario: No main conversion owns unresolved images
- **WHEN** the prepared transport is known text-only but main context has unresolved images and no conversion is in progress
- **THEN** /btw fails as context not ready without triggering recovery or modifying image capability

#### Scenario: Historical images rejected by side question
- **WHEN** its frozen request receives an image rejection or ordinary 400
- **THEN** it records the Sideband failure and returns it without starting shared image recovery or writing negative capability

#### Scenario: Foreground context advances during the failed request
- **WHEN** /btw's frozen provider request fails while main context advances
- **THEN** /btw returns the failure without projecting either its frozen images or images from the newer Surface

### Requirement: Skill publication uses projected request context pressure

User and model skill publication SHALL check existing context using ChatState's current provider-anchored request projection, including committed Surface deltas and the current request envelope adjustment. The commit SHALL add the estimate of all complete new user/result/skill items and recheck the current Surface revision and reserved headroom. Raw history omitted by provider projection SHALL NOT independently cause rejection; known provider overhead SHALL NOT be discarded. The dependent request's final projection and budget validation SHALL remain required.

#### Scenario: Raw history exceeds projected request size

- **WHEN** raw history includes reasoning omitted by the request projection and the current projected context plus a complete skill group fits the reserved budget
- **THEN** the group may publish atomically without rejecting solely because the raw Surface estimate is larger.

#### Scenario: Provider accounting exhausts available space

- **WHEN** the current provider anchor plus complete new skill content exceeds reserved headroom, even without a Surface revision change
- **THEN** user publication rejects without consuming or exposing any part of the group, and model publication uses its existing paired rejection result without publishing skill guidance.

### Requirement: Cold context pressure follows portable replay

When cold restoration starts a new native continuation lane, ChatState SHALL initialize context pressure from the provider-neutral portable history that this lane can replay, without counting omitted historical reasoning. It SHALL retain effective input and request-envelope estimates in the same coordinate. The first real request SHALL add its current request envelope through the existing projection path, and later provider observations SHALL remain authoritative anchors. Initialization SHALL NOT delete history, start provider work, fabricate usage, or clamp genuine overflow to the model window.

#### Scenario: Resume retains reasoning that is omitted from replay

- **WHEN** a restored Timeline contains enough historical reasoning to make its raw estimate exceed the window, while portable message content fits
- **THEN** context pressure reflects the portable message estimate before the first prompt, and the raw reasoning remains available in Timeline.

#### Scenario: A new message follows restoration

- **WHEN** a committed Surface append and a normal request build follow cold initialization
- **THEN** the new content is counted once, and request projection replaces the prior envelope adjustment without reintroducing omitted reasoning or subtracting it twice.

#### Scenario: Portable content is genuinely too large

- **WHEN** portable replay itself exceeds the configured model window
- **THEN** the restored context count remains above the window rather than being clipped for display.

#### Scenario: Restored active Goal is visible before the first request
- **WHEN** 冷恢复的控制授权为 Goal 模式且目标状态 Active，尚未发出模型请求
- **THEN** 首次上下文快照保留匹配该 id/revision 的最新指令，shadow 其他指令；不通过历史最后 tag 猜授权，不启动 provider；已完成/暂停/缺失目标不保留活动指令。

#### Scenario: Recovery reconciliation pauses the Goal
- **WHEN** 初始持久 Goal 为 Active，但恢复协调因执行条件不可用将其暂停
- **THEN** 会话暴露和首次请求前的最终压力按暂停后的授权重新同步，不残留活动 scope。

### Requirement: Rewind and route replacement rebase context pressure to portable replay

After a durable conversation rewind or a sampling route replacement, ChatState SHALL initialize context pressure and request-estimate baselines from the new continuation lane's portable history, using any portable reasoning rule retained by that lane. It SHALL publish the resulting pressure before a new provider request. Raw retained reasoning omitted by replay and the previous branch or route's provider anchor SHALL NOT inflate the new count. Subsequent mutations and request envelopes SHALL use effective input estimates with same-route provider calibration, without raw Surface delta adjustments.

This initialization SHALL NOT change cumulative consumption or durable conversation content, start provider work, or clamp genuine portable overflow. Failed rewind SHALL NOT reset pressure. Ordinary sampling parameter updates and non-rewind compaction SHALL retain their existing provider-anchor behavior.

#### Scenario: Rewind crosses compaction before switching models

- **WHEN** rewind restores pre-compaction history with large reasoning omitted by portable replay and the user then replaces the model route
- **THEN** rewind and route replacement each publish portable context pressure, preserving original history, and the first and repeated requests do not reintroduce the omitted reasoning.

#### Scenario: Route replacement invalidates the previous provider anchor

- **WHEN** the selected provider, model or backend route is replaced after a provider context observation
- **THEN** the counter immediately reflects portable history before the first new request, and later provider observations calibrate it normally.

#### Scenario: Portable content really exceeds the new window

- **WHEN** portable history itself exceeds the new model's context window
- **THEN** the count remains above that window rather than being capped for display.

#### Scenario: Failed rewind and ordinary configuration updates

- **WHEN** rewind is rejected or cannot durably commit, or only same-route sampling parameters change
- **THEN** the current context pressure and provider calibration remain intact.

### Requirement: Context mutations use effective request estimates
ChatState SHALL compute context pressure from the effective current request input estimate with the current route's provider calibration. Raw Surface content omitted from replay SHALL NOT be subtracted from provider-visible pressure. Surface mutation, request assembly, native reset and learned reasoning changes SHALL preserve this coordinate; repeated request projection SHALL be idempotent. Route replacement and rewind SHALL reset calibration; other replacements SHALL retain valid same-route calibration. Failed durable writes SHALL NOT publish changed pressure. Lifetime usage SHALL remain independent.

#### Scenario: Compaction removes mostly omitted reasoning
- **WHEN** raw removed history exceeds the current pressure while portable retained history is nonempty
- **THEN** pressure reflects retained effective content and overhead without an intermediate saturated subtraction, and a later request cannot recover phantom tokens.

#### Scenario: Explicit request projection changes
- **WHEN** an image route invalidates cached selection, native continuation resets, or portable reasoning replay changes
- **THEN** context pressure is recomputed immediately and a changed count is published before another request is built.

#### Scenario: Projection and mutation boundaries
- **WHEN** append, pruning, image projection, repair, schema changes or repeated builds alter the effective request
- **THEN** changed content is counted once, omitted history stays omitted, and valid provider overhead is preserved.

#### Scenario: Active Goal projection survives image cache invalidation
- **WHEN** 当前请求保留活动 Goal 的长指令，模型路由或图片缓存变化但 Goal 授权范围未变化
- **THEN** 重算压力仍保留当前 Goal 指令并 shadow 已过期指令；不能因图片缓存被清除而遗漏当前指令；随后构建相同作用域请求时计数保持一致。

#### Scenario: Complete provider context is lower than the heuristic
- **WHEN** 已接纳响应提供完整可信 context count，数值低于当前有效本地估值
- **THEN** 压力精确接纳该锚点，消费账本不因此改写；后续追加按本地增量计入，缩小 footprint 按当前校准参考比例重算，不以负差分饱和丢失保留内容；重复 build 不重复调整。

#### Scenario: Low provider calibration survives multiple compactions and regrowth
- **WHEN** 低值provider锚点后执行多次合法压缩、增长和重复请求构建
- **THEN** 各次变化使用最新滚动参考，追加计入完整本地增量，正的保留footprint不被负差分算成零，重复构建和失败写入不重复调整，累计消费不变。

#### Scenario: Restored Goal crosses route and rewind in either order
- **WHEN** 冷恢复的当前Goal授权不变，模型切换与用户rewind先后请求，历史含其他Goal标签
- **THEN** 用户入口按既有Goal约束拒绝rewind并保留历史/压力；新lane清旧校准但按当前授权投影。独立的内部durable历史重放不能从恢复的旧标签激活旧Goal。

#### Scenario: Clearing a restored Goal updates pressure before route or rewind
- **WHEN** 冷恢复后 `/goal clear` 已持久化并清除运行时授权，随后切模型或用户 rewind
- **THEN** 在下一次请求构建或命令状态公布之前，ChatState 作用域同步为 None；恢复到历史 Goal 标签不重新激活授权，累计消费保持不变；clear 的持久化失败不清除原授权和投影。

### Requirement: Known input overflow cannot bypass provider admission
Before ordinary provider dispatch the Session SHALL check the current calibrated input pressure plus the actual explicit output limit against its current configured window independently of auto-compaction suppression and task output budgeting. Model-switch compaction errors SHALL propagate through existing cancellation and failure boundaries rather than authorize sampling. Automatic recovery MAY attempt existing bounded compaction; a still oversized request SHALL fail without provider dispatch.

#### Scenario: Smaller model and failed compaction
- **WHEN** 600K context on a 1M model switches to a 500K model and compaction fails or leaves a known overflow
- **THEN** the turn fails without sending the ordinary oversized request, even if compaction suppression is active.

#### Scenario: Suppression or child budget disables automatic compaction
- **WHEN** a fully projected ordinary request exceeds its configured window and automatic compaction is unavailable
- **THEN** provider dispatch is rejected while existing history and usage remain intact.
#### Scenario: Explicit output limit consumes the same window for ordinary and skill requests
- **WHEN** 普通或带技能请求的校准输入符合窗口，但加上 task clamp 后实际显式输出上限会超过总窗口
- **THEN** 两者均按同一总预算准入，无法恢复时在 provider 派发前拒绝；相等边界可发送；不添加未声明的固定输出预留或额外 256。

#### Scenario: Provider output limit is implicit
- **WHEN** 请求未设置显式输出上限且路由未提供该上限
- **THEN** 最终准入检查已知输入，不虚构某后端的默认输出值；本地无法据此保证未知输出预算。

#### Scenario: Explicit output overflow enters existing automatic precompaction
- **WHEN** 已投影输入低于普通压缩阈值但加实际显式输出预留超过窗口，输出预留小于窗口且既有自动压缩准入允许
- **THEN** 先跨闭合Step边界执行一次有界预压缩，prune和图片重投影按相同总预算复核；重新投影后只有总需求符合窗口才发送普通请求。

#### Scenario: One compaction cannot fit the reserved output
- **WHEN** 为输出预留触发的压缩或后台提升已完成，但总需求仍超窗
- **THEN** 保留已提交压缩及其Completed，结束本轮且不派发普通请求，不反复重发摘要；自动恢复被抑制或受子任务输出规则禁用时仍由最终准入拒绝。

### Requirement: Side question work follows its response receiver

A `/btw` side-question execution SHALL remain active only while its response receiver is open. Closing the receiver before execution starts SHALL prevent the question from starting; closing it during context preparation, Sideband execution, provider work, or retry backoff SHALL cancel the in-flight question. Cancellation SHALL release its execution ownership and preserve existing Sideband usage settlement and terminal cleanup. Session-level Sideband cancellation SHALL retain its existing behavior.

#### Scenario: Receiver closes before the worker starts

- **WHEN** a SideQuestion command's response receiver closes before its detached worker is first polled
- **THEN** the worker does not start context or provider work and creates no Sideband

#### Scenario: Receiver closes during side-question work

- **WHEN** the response receiver closes while the question is waiting or executing
- **THEN** the in-flight future is dropped, its Sideband-owned admitted usage is settled and terminal cleanup proceeds, with no further retry or provider work

#### Scenario: Session cancellation remains effective

- **WHEN** the caller still awaits the answer but the owning session cancels Sideband work
- **THEN** the question follows the existing session cancellation path and returns its existing cancellation failure when the receiver remains open

### Requirement: Durable control transitions update Goal request scope atomically
A Session control transition SHALL publish its authorized Goal request scope through the same ChatState durable commit boundary as its Control fact. Scope SHALL follow active Goal identity/revision and selected Behavior before another request or context advertisement. Failed Control writes SHALL NOT publish a changed scope or pressure; route changes SHALL retain the current committed scope while clearing provider calibration. Lifetime usage SHALL remain unchanged.

#### Scenario: Stop or leave the active Goal
- **WHEN** pause, completion, budget stop, clear or a Behavior transition durably removes active Goal authorization
- **THEN** pressure before the next request uses no active Goal scope, without a transient old-scope update from the new Control content.

#### Scenario: Restart or change definition
- **WHEN** a durable Control activates a Goal or changes its active definition revision
- **THEN** the new tag governs context projection before the next build and survives model-route replacement.

#### Scenario: Persistence does not confirm control
- **WHEN** a proposed Control fails validation or durable persistence
- **THEN** no changed request scope/pressure is published; permanent storage failure retains existing owner shutdown and cold recovery semantics.

#### Scenario: Request projection overlaps a Goal control
- **WHEN** Goal scope capture and ChatState request preparation overlap a pause, clear or definition transition
- **THEN** the same existing Step control gate serializes scope capture with preparation so an older captured tag cannot overwrite the committed Control scope after an asynchronous image-mode query.

### Requirement: Local OCR owns its helper process tree

Local OCR SHALL execute its helper in a process group/job whose lifetime is owned by the OCR operation. Timeout, cancellation, dropping the operation, or normal completion SHALL terminate any remaining helper descendants. The direct helper SHALL retain the existing kill-on-drop and wait/reap behavior; normal completion SHALL preserve bounded input/output behavior and recognized text.

#### Scenario: OCR operation is cancelled

- **WHEN** the caller cancels or drops local OCR while its helper is running
- **THEN** the helper process group/job is terminated, including descendants, and the direct child remains subject to the existing wait/reap behavior

#### Scenario: OCR deadline expires

- **WHEN** the local OCR deadline expires while the helper or a descendant is keeping the operation pending
- **THEN** the owned process group/job is terminated and the caller receives the existing timeout result

#### Scenario: OCR completes with a detached descendant

- **WHEN** the direct helper returns valid text while a descendant remains in the process group with its output pipes closed
- **THEN** local OCR returns the recognized text and terminates the remaining process group before releasing ownership
