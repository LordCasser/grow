## 1. Implementation

- [x] 1.1 核对 prompt parser、现有 notice helper、Timeline 消息与 gateway 通知边界。
- [x] 1.2 从真实 prompt 入口建立内嵌图片丢弃/压缩/正常/混合的失败回归。
- [x] 1.3 复用 normalize_images_with_notices，保留图片提取、权限文本与原有批次边界。

## 2. Verification and documentation

- [x] 2.1 验证新增回归及相关图片/admission 测试，检查 fallback 仍由同一个 helper 处理。
- [x] 2.2 更新开发说明和 backlog，记录验证结果及限制。
- [x] 2.3 完成归档准备：严格 OpenSpec 验证并统一清理本轮构建产物；归档结果另记 verification。
