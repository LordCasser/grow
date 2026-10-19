## Implementation

- [x] 1.1 在共享图片拒绝 matcher 中识别仅允许 `text` 的 `messages.content.type` 列表声明，沿用 HTTP 400、实际图片和现有排除条件。
- [x] 1.2 为 matcher 增加正反矩阵，覆盖单双引号、尾部空白/标点、相近列表及负向边界。
- [x] 1.3 覆盖 sampler 对该共享分类结果的 exact-zero usage 行为（若适用）。
- [x] 1.4 添加 Shell Chat 错误回归，验证复用现有视觉辅助/OCR、durable `ImageProjection` 与 ACK 后请求重建流程。

## Validation

- [x] 2.1 运行相关 sampling-types、sampler 和 shell 回归验证。
- [x] 2.2 确认原始 Timeline 事实、session 文件和安装二进制均未被修改。
