## 1. 实现与回归

- [x] 1.1 泛化明确 reasoning 拒绝事实与路由状态，验证 Chat/Responses 分类、DTO 往返、backend 不匹配拒绝、ACK 幂等与 route reset。
- [x] 1.2 实现 Chat assistant reasoning 编码，验证多段 reasoning、无 reasoning、边界、工具配对、native 保留与三协议隔离；先保留失败回归结果。
- [x] 1.3 接通 Shell 既有恢复分支，验证首次恢复及重复拒绝终止、模型切换回归与共享预算行为。

## 2. 验证与归档

- [x] 2.1 更新开发说明，运行受影响 crate 测试及 Shell 目标测试，记录真实会话证据和未执行验证。
- [x] 2.2 执行全量 strict OpenSpec 校验、归档并再次执行全量及 archive 校验，核对 diff。
