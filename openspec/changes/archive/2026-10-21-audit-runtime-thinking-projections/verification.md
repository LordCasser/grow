# 验证记录

2026-10-21，Rust/Cargo 使用仓库固定的 1.93.1，CARGO_BUILD_JOBS=2。离线数据均为审计自造短文本，不调用 provider。

## 执行

1. 将 `harness/runtime_projection_audit.rs` 临时复制到 `crates/codegen/sampling-types/tests/runtime_projection_audit.rs`。
2. `cargo test --locked -p sampling-types --test runtime_projection_audit -- --test-threads=4`：7 passed，0 failed / ignored。
3. 将 `harness/skill_repair_audit.rs` 临时复制到 `crates/codegen/chat-state/tests/skill_repair_audit.rs`。
4. `cargo test --locked -p chat-state --test skill_repair_audit -- --test-threads=2`：2 passed，0 failed / ignored。

这 9 项是观察性探针，断言当前缺陷形状或已修复对照，不能把“passed”解释成产品无缺陷。两项 adapter 批次反例的生产适用边界详见 audit.md；未运行完整 Shell actor E2E，也未做真实 API 400 重现。

审计完成后移除两个临时 Cargo target，仅在本归档 harness 保留可复现源码；生产源码保留此前工作树改动。本次没有修改实现，不增加固定维护错误行为的产品测试。

OpenSpec 归档前 strict 全量 19/19；归档后 strict 全量 18/18；archived 680/680。`git diff --check` 本次文档通过。验证后 `cargo clean -p sampling-types -p chat-state` 清理相关 crate 编译产物，保留共享依赖缓存。
