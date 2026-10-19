## Context

共享 matcher `is_unconditional_image_input_unsupported` 被 sampler 的精确零用量归类和 Shell 的图片恢复共用。它识别英文 unconditional image claims 和明确 image 字段，但不识别中文错误 `messages.content.type 参数非法，取值范围 ['text']`。因此在有图且 HTTP 400 的请求上，当前能力拒绝被当作普通 fatal，Shell 没有启动已有图片处理链。

## Decision

在共享 matcher 中增加一个窄的终态 claim：`messages.content.type` 的允许值列表只包含 `text`。支持列表值的单引号与双引号形式，并允许该声明后的尾部空白或终态标点。它仍须满足现有 HTTP 400 与 `image_count > 0` 门槛，且受现有通用参数错误排除条件约束。

仅将该 claim 作为既有 unsupported-image 分类证据。分类成功后复用已有视觉辅助模型 → local OCR → durable `ImageProjection` → 基于 acknowledged projection 重建请求；不新增 fallback、投影种类或重试路径，不按 model/provider 名称预判。

其他 content type 列表（包括 `['text', 'image_url']`）、列表后追加 `or image...` 的宽泛说明、空列表、一般 schema/type 参数错误、非 HTTP 400 以及无图请求均不得匹配。原始 Timeline 与错误事实保持不变；已有 projection 失败保护继续生效。

## Verification

为共享 matcher 建立正反矩阵，覆盖引号、末尾空白/标点、相近但不等价列表、schema/type 错误、状态码和图片数量边界。sampler 覆盖该 claim 下既有 exact-zero 归类（适用时）；Shell 使用实际中文 Chat 错误验证进入既有图片恢复链，并检查 projection ACK 后才重建请求。验证不修改既有 session 文件或安装二进制。
