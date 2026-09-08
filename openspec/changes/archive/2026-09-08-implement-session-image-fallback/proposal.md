## Why
Make image sampling fallback explicit per session model route. Requested as part of the second cleanup batch before republishing v2.1.5.

## What Changes
Send original image content by default. On a confirmed unsupported-image response, show 当前模型不支持多模态，调用视觉辅助LLM处理中... when a visual auxiliary model is available. Describe images using the configured visual auxiliary LLM; if absent or failed show 视觉辅助模型未配置或者调用失败，使用OCR处理中... and try local OCR. Keep image plus description in Grow image data. Retry the primary model using descriptions; mark the session provider/model tuple to use text on subsequent assemblies. An unmarked new tuple first tries images. Reuse model-switch context projection and existing image/description storage where possible. Audit R27 normalization-cache deletion separately from necessary normalization and concurrency limits.
