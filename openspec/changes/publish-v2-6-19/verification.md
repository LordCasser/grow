## 本地候选

- 远端最新正式版为 v2.6.18，v2.6.19 在本地和远端均未占用。Workspace 与锁文件中 33 个本仓库包的版本同步为 2.6.19。
- Rust 1.93.1 下 Rustfmt、locked metadata、默认 CLI 检查、全工作区 all-targets 检查、release-dist 的 `-D warnings` 检查通过。全目标检查保留了既有测试警告。
- 发布及测试执行脚本 34 项通过，1 项按平台跳过。OpenSpec 严格校验 19 项、归档校验 992 项通过。
- 本次候选纳入上下文审计和已验收的修复。公开审计文档用代号替换现场会话和网关历史索引。测试计数、复现和集成边界见[上下文修复验收](../archive/2026-11-25-close-context-management-audit/verification.md)。
