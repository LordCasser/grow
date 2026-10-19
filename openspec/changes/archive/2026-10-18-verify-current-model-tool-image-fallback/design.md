## Context

Loki 证据显示：模型在无图请求上成功后，历史 `read_file` tool result 含一张图片，后续同一 `glm-5.3` Chat route 的请求重复携带该图片并收到 text-only content-type 400。该 session 没有 image-description Sideband 或 ImageProjection，表明 fallback 未进入，不能归因为视觉辅助调用失败。

源码中的 text-only claim 识别已由 `recognize-text-only-content-type-rejection` change 覆盖并归档。本 change 只在现有会话级图片恢复测试中补当前模型同路由场景，复用现有 visual auxiliary → durable projection → request reconstruction 路径；不增加 matcher、fallback、路由预测或产品代码。

## Verification Design

测试在已存在的历史 tool result 图像和当前 Chat route 下发起请求，模拟实际中文错误，再提供辅助模型描述和主模型重试响应。断言初始请求携带原图、辅助请求获准且收到图像、durable 投影可回放、重建请求不含图像并包含描述、原始 Timeline 图像仍保留、Goal usage 未变为 incomplete。

该场景核验当前模型实际返回拒绝后的既有恢复；unknown route 首次仍发送原图的契约继续由现有规范定义。本 change 只读核对真实 session 证据，不修改 session 文件，不安装或启动本地 Grow 程序。
