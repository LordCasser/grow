## 1. 审查基线与架构图

- [x] 1.1 核对当前 `main` 与历史 inventory 的源码漂移，确认旧的逐包完成标记不足以证明当前全仓完成。
- [ ] 1.2 从当前真实入口、模块边界、状态所有权、依赖方向和测试体系形成 Grow 架构总图。

## 2. 按特性审查

- [ ] 2.1 补录直接父子协调、工具授权和输入准入三个已完成切片的当前证据与结论。
- [x] 2.2 完成 `model-sampling/attempt-admission`：provider attempt 身份、共享恢复预算、取消、候选接纳、工具 authority、用量、terminal、replay 和客户端投影。
- [ ] 2.3 继续审查尚未覆盖的行为特性，直到当前源码的相关能力均有权威证据。
- [x] 2.4 复核 v2.1.10 已合并的响应恢复提交，记录连续 rewind、fork、resident snapshot 和 durable ACK 边界的问题及证据；修复保持独立。

## 3. 验证与交付

- [x] 3.1 完成 model-sampling 阶段的定向测试、OpenSpec 严格校验和磁盘检查；验证结果记录在 `verification.md`。
- [ ] 3.2 最终综合全部 findings、覆盖缺口和架构图，重新核对源码漂移及全仓验证边界。
