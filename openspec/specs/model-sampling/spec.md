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
