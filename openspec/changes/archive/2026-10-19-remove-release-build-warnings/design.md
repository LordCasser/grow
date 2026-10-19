## Scope and invariants

以 Rust 1.93.1 诊断和真实调用方确定代码的编译边界。`prompt_status_query_matches_for` 与 `prompt_status_query_matches_generation_for` 是生产入口；它们接受无 prompt ID 的 lifecycle 查询，不能为了消除 warning 改回只接受 prompt ID 的路径。两个简写包装仅为同 crate 单元测试服务，应与相邻测试辅助方法一样使用 `#[cfg(test)]`，而不是 `allow(dead_code)`。

先完成未修改源码的 CLI 编译以保留基线，然后核对工作区 target、默认 CLI 及 release-dist feature/profile 的诊断。现有回归应继续检查旧 generation、取消、重放和 lifecycle 结果归属。若诊断发现别处的确定性编译 warning，只按其调用和 cfg 修复，不扩展到重构、依赖升级或 Clippy 风格清理。

本地 host 编译不能证明十个平台全部无 warning；Windows、Linux 与 OHOS 的 SDK、原生库和工具链仍由发布矩阵验收。不改动独立 MCP 分支，不发起发布。
