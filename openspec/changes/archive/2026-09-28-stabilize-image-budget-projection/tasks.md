## 1. 复现与实现

- [x] 1.1 在 actor 真实 build 路径补首次回收后至少两轮追加的回归，验证旧图片选择逐轮变化的失败现场，并记录原始 Timeline、epoch 和 key。
- [x] 1.2 在现有 actor 请求投影状态保留图片选择，按源身份校验并先重放后测量；通过大图片与多张小图片的两次高水位回归。
- [x] 1.3 实现 design 中的投影域失效矩阵；验证 native reset 保留有效选择，rewind/compaction/route/description/goal 域变更不套用旧 part 身份。
- [x] 1.4 更新 image_budget 证据与注释，核对重复 build 与新增淘汰为零时仍能解释完整生效选择。

## 2. 验证与收尾

- [x] 2.1 覆盖 User/ToolResult 多 part、三协议附件编码、fork/冷恢复/resident reconnect 和最终 wire 超限；定向测试证明原始图片证据保留且失效的 native 不复活。
- [x] 2.2 更新 `docs/development.md` 的请求图片预算说明和规范链接；在 `verification.md` 记录实际命令、结果、未执行范围及本次 Cargo 产物处理。
- [x] 2.3 逐项核对 delta 场景后运行 `openspec validate --all --strict --no-interactive`，仅在实现与验证完成后归档本 change，再执行全量规范及 `openspec validate --archived --no-interactive`。
