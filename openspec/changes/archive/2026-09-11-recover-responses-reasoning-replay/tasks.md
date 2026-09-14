## 1. 契约与复现

- [x] 1.1 核对模型切换、portable 投影、错误 DTO 和恢复预算的完整数据流。
- [x] 1.2 新增 queued route switch 后 Responses 工具历史缺少 reasoning 的失败回归。

## 2. 实现与验证

- [x] 2.1 增加明确 400 的类型化分类与 DTO 往返测试。
- [x] 2.2 实现当前 route 的 acknowledged 兼容学习及 Responses-only portable reasoning 投影。
- [x] 2.3 将恢复接入现有 logical sampling budget，验证首次修复、重复拒绝终止及 route 切换隔离。
- [x] 2.4 运行受影响 crate 回归并记录结果。

## 3. 文档与归档

- [x] 3.1 更新采样恢复开发者说明和 verification，逐项核对场景。
- [x] 3.2 strict 校验后归档 change，再校验主规范与 archive。
