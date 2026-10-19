## Regression coverage

- [x] 1.1 在现有 `image_input_recovery_tests.rs` 增加当前模型 Chat route 的 historical tool-image 中文 400 回归，并复用既有场景 helper。
- [x] 1.2 覆盖视觉辅助、durable `ImageProjection`、无图重建请求、原图保留及 Goal usage 完整性。

## Validation and evidence

- [x] 2.1 运行主实施路径指定的定向验证并记录结果。
- [x] 2.2 复核 Loki request/response、Sideband 与 projection 事实，保留 session 文件只读。
- [x] 2.3 确认不安装或修改本地 Grow 二进制，不改变 unknown-route 首次发送原图的行为。
