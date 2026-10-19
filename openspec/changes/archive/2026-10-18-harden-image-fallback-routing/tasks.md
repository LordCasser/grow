## Transport identity

- [x] 1.1 为 `ModelImageInputKey` 增加稳定的标准 JSON storage key，并保持 `ModelImageInputState` 的字符串集合存储形状。
- [x] 1.2 将 Shell 的 primary/auxiliary 拒绝查找和记录改为接受已解析的 typed transport key；compaction 使用相同身份，pending 拒绝恢复须验证完整 transport。
- [x] 1.3 确认拒绝事实只由实际 transport key 决定，UI 标签仍显示 catalog identity，旧 alias 值不被迁移或清理。

## Regression coverage

- [x] 2.1 覆盖不同 alias 指向同 runtime 时共享拒绝，以及同 alias 的 endpoint、backend 或 query route 改变时不继承旧拒绝。
- [x] 2.2 覆盖 configured auxiliary 重新解析到新 transport 后确实调用新路由，以及 auxiliary 与 rejected primary 相同时避免递归。
- [x] 2.3 覆盖 primary、auxiliary 与 compaction 的 runtime key 一致，并复用现有图片 fallback、描述缓存和 projection ACK/失败回归。

## Validation

- [x] 3.1 运行受影响 crate 的定向测试并记录 identity、fallback 和 durable projection 结果。
- [x] 3.2 对直接 user image、read_file/PDF image、历史图像、模型切换、auxiliary 配置、OCR、失败、取消及 deadline 场景完成分层覆盖审查。
