## Why

Grow 已有成熟的 unsupported-image fallback：确认拒绝后复用视觉辅助模型、local OCR、durable `ImageProjection`，再按投影重建请求。但共享错误识别器没有覆盖一种明确的 text-only 能力声明，导致真实的带图请求落入 terminal failure，未进入现有 fallback。

已确认的只读证据来自 Loki session `01a14a92-2758-7ac3-8c08-ceb224b5490d`：Timeline seq 15471 的 Chat 请求使用 `glm-5.3`，route 为 `open.bigmodel.cn/api/coding/paas/v4/chat/completions`；1710 条 messages 中有 19 个历史 `tool` 角色 `image_url`。seq 15472 原始响应为 96 bytes：`{error:{code:"1210",message:"messages.content.type 参数非法，取值范围 ['text']"}}`；seq 15474 记录 fatal，未进入图片 fallback。

## What Changes

- 扩展共享 unsupported-image 错误识别器，识别精确的 `messages.content.type` 可选类型列表仅含 `text` 的终态声明，包括单引号或双引号及末尾空白/标点。
- 继续要求 HTTP 400 且请求确实含图片，并保留现有排除门槛；只有被确认的拒绝继续流入既有视觉辅助、OCR、durable `ImageProjection` 和请求重建流程。
- 不按模型名预判能力，不新增 fallback，不改变已有 session 内容或安装二进制。

## Capabilities

### Modified Capabilities

- `model-sampling`：将该明确的 text-only content type 列表声明加入现有 unsupported-image rejection 分类条件，沿用既有图片投影与恢复契约。

## Impact

改动限于共享错误分类器与既有采样/会话恢复调用路径的回归覆盖。需覆盖 matcher 正反矩阵、sampler 的精确零用量行为（若适用）和 shell 使用实际 Chat 错误进入现有 fallback 的回归。开发者说明由主 change 维护。

## Evidence and Status

本 change 以保存的 Timeline request/response/error facts 为依据；session 文件仅作只读证据。共享 matcher 已加入精确 text-only content type 声明分类，并复用既有 HTTP 400、图片数量和验证/策略排除门槛；已有正反矩阵覆盖单双引号、尾部空白/标点、近似列表及边界。相关测试尚未执行，sampler exact-zero 与 Shell 恢复回归仍待验证。现有视觉辅助、OCR、ImageProjection、重建请求及其持久确认契约继续适用。
