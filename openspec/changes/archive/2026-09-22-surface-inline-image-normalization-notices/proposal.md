# Change: Surface inline-image normalization notices

## Why

Backlog 记录的 user query 图片入口仍直接调用 `normalize_images`，只取 surviving images，并将 re-encode fallback 写日志。用户在文本中粘贴的 base64 图片被丢弃或压缩时，UI 与模型输入都缺少处理结果；ACP attachment 已使用 `normalize_images_with_notices`，两种输入形式出现不一致。

## What Changes

- 文本内提取的图片复用现有 normalization + notice 所有者。
- 丢弃、压缩和保留原图的 fallback 结果进入当前 prompt 的文本 context，并发布对应现有 `ImageDropped` / `ImageCompressed` 通知。
- 正常图片继续进入模型消息；图片二进制不混入 notice 字符串，不改变提取、预算和格式校验规则。
- 从真实 prompt admission 入口验证 dropped / compressed / healthy 输入和混合图片。

## Capabilities

### Modified Capabilities

- `input-admission`: 明确从 query 文本提取的图片也保留 normalization 结果证据。

## Impact

限于 Shell `turn/admission.rs` 的既有入口和对应测试、开发说明。不引入新通知类型、存储 schema、图片处理器或跨入口全局批处理。原有 ACP attachment 与 query extraction 仍各自构成一批，索引分别属于输入批次。
