# Grow 2.5.1 发布验证

## 发布准备

2026-10-25（Asia/Shanghai）。当前组合工作树含五项已归档行为变更，纳入 2.5.1。对照主规范、实现调用链和已有回归记录，未扩展未完成的 MCP 交互提案。

- Rust/Cargo 固定 1.93.1（显式将 rustup 代理置于 PATH 首位）；`cargo metadata --locked --no-deps --format-version 1` 通过，CLI/workspace 为 2.5.1，33 个继承版本的本地 lock 包同步。
- `cargo fmt --all -- --check`、`git diff --check` 通过。
- `python3 -m unittest discover -s scripts/tests -p 'test_release*.py'`：13 项，12 passed / 1 skipped；未把平台跳过计为通过。
- `openspec validate --all --strict --no-interactive`：17 passed；`openspec validate --archived --strict --no-interactive`：709 passed。补足 busy effort 归档后组合状态的校验。
- 复核 Goal completion_report 的输入/authority/交付门槛、Control 持久化 ACK 与失败回滚、typed history 和 live/replay 展示；上下文恢复保留 raw history、真实 overflow 和追加差值；技能发布使用同一 projected_tokens；忙碌 effort 只结算匹配 intent；/btw 遮挡、链接与历史查看均有现有回归。

本地已有五项专项验证不冒充最终发布 SHA 的全平台验证。官方工作流需在最终标签提交上执行全部必需回归和十平台构建，全部通过才发布；发布结果与公开包验证稍后补记。
