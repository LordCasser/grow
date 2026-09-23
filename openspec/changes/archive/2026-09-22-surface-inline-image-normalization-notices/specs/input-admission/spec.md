## ADDED Requirements

### Requirement: Inline query images retain normalization outcomes

Prompt admission SHALL 使用现有图片 normalization notice 入口处理从 query 文本提取的图片。每个内嵌图片批次的丢弃、压缩和保留原图 fallback 说明 SHALL 同时进入当前模型可见 prompt context 与对应 Session 的现有图片通知；索引和同原因分组 SHALL 沿用 normalization 结果。有效图片 SHALL 保留原顺序，且 notice 文本 SHALL NOT 包含图片二进制内容或被用作新的用户授权。

#### Scenario: Inline images are dropped

- **WHEN** 同一 query 中多个提取的图片因为相同原因被 normalization 删除
- **THEN** 当前用户消息保留一次包含全部受影响索引的分组说明，UI 收到一份相同原因的 ImageDropped 通知，被删除的图片不进入模型图片列表。

#### Scenario: Inline image is compressed

- **WHEN** 提取的图片经 normalization 压缩后仍有效
- **THEN** 模型消息保留处理后的图片及压缩说明，UI 收到对应 ImageCompressed 通知。

#### Scenario: Re-encoding falls back to the original image

- **WHEN** 提取的图片无法重编码到目标预算而 normalization 保留原图
- **THEN** 原图与 indexed fallback 说明都保留，UI 使用既有 fallback 通知语义，不仅记录日志。

#### Scenario: Healthy and mixed inline images

- **WHEN** query 含正常图片，或同时包含正常与被删除图片
- **THEN** 正常图片按原顺序保留；只有实际发生的处理结果生成说明，无变化的图片不产生压缩或丢弃通知。
