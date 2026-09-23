# Change: Recover sessions after a model rejects image input

## Why

当前图片恢复链路有两个独立问题。

第一，图片规范化已经逐张识别并删除过小、损坏等无效附件，但它把每个结果提前渲染成完整句子。一次工具快照包含多张同类小图时，`ImageDropped` NOTICE 和模型可见 reminder 会逐句重复相同原因，噪声随图片数线性增长。

第二，也是更严重的问题：文本模型可能以 `400 InvalidParameter: <model> is not a multimodal model` 明确拒绝仍存活的图片。共享分类器不识别这类 provider-neutral 的终态能力声明，因此不会记录 text-only 能力、不会启动图片投影，也不会重试。原图继续存在于当前 Surface；以后即使用户只发送文本，请求组装仍会带上该历史图片并重复同一个 400，使 session 实际不可继续。

现有“视觉辅助和 OCR 都失败时保留原图并终止当前请求”的契约也无法满足恢复目标：原始 Timeline 证据确实安全，但同一张图片仍在模型可见 Surface 中，所谓 recoverable failure 对当前 text-only route 并不可恢复。

## What Changes

- 图片规范化保留逐图判定，但在渲染前按相同原因聚合删除结果；一次 normalization batch 只发一条 `ImageDropped` 更新和一个 reminder，相同原因只出现一次并列出相关图片序号。
- 共享 unsupported-image 分类器识别带 provider/model 前缀的终态 `is not a multimodal model` 400，同时继续排除尺寸、格式、损坏和策略错误。
- 扩展既有 `ImageProjection`，使每个 image group 可以表示“已生成描述/OCR”或“模型不支持且无可用文本回退”。后一种投影在 durable ACK 后从当前 Surface 删除原始图片，并在该 group 的首个图片位置放入标准文本 `当前模型不支持多模态，图片已经被删除`。
- 原始消息事件和图片 payload 继续作为不可变 Timeline 证据保留；删除只作用于当前 branch 的 materialized Surface、相关 compaction 引用以及图片工具调用的可再注入路径。
- 视觉辅助与本地 OCR 的成功路径保持；只有仍未解决的 group 才使用删除投影。投影提交成功后才重建并重试 primary request，保证本次重试和后续纯文本 turn 都不再携带已删除图片。
- 投影校验、持久化或 ACK 失败时 fail closed，不允许只修改内存请求后重试；已有 poisoned session 在升级后再次收到同类明确拒图 400 时，可以通过同一 durable projection 自动恢复，无需 rewind。

## Capabilities

### Modified Capabilities

- `model-sampling`: 聚合图片删除通知，识别 multimodal 能力拒绝，并在视觉描述/OCR 均不可用时提交删除投影后重试。
- `session-timeline`: `ImageProjection` 新增可回放的 unsupported-image 删除语义；Timeline 保留原始证据，Surface 持久移除会毒化后续请求的图片。

## Impact

- 主要实现涉及 `crates/codegen/shell/src/session/image_normalize.rs`、tool/user attachment admission、`sampling-types/src/conversation.rs`、Shell sampling recovery，以及 chat-state 的 `ImageProjection` 校验、apply、branch replay、token/continuation 更新与 storage consistency check。
- `GrowSessionUpdate::ImageDropped` 仍使用既有 `notes` wire shape，pager 仍按一次更新生成一个 NOTICE block；聚合在规范化边界完成，不新增 UI schema 或第二套 renderer。
- 需要覆盖精确 GLM 错误、相似但非图片能力错误、无辅助模型/OCR 失败、混合 description/removal group、工具结果图片、cold replay、后续纯文本 turn 和持久化失败注入。
- 不改变未知模型首次发送原图的 optimistic 策略，不把预先模型目录判断作为图片删除 authority，也不清理无关的通知或通用 provider retry 架构。
