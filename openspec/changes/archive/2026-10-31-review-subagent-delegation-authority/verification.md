# Verification

## Method and limits

临时将 `probe.rs` 接入 `crates/codegen/shell/src/agent/subagent/tests/mod.rs` 的子模块，使用生产 resolver、gate、目录发现、AgentRebuildSpec、执行 recipe 编解码和 capability ceiling。探针断言的是当前缺陷形态，不作为修复后应保留的绿色行为契约。最终移除接线，仅保留审计用源码。

普通路径和插件别名探针分别核对实际文件/selector identity，避免把不同 Agent 的权限差别误报为绕过。CLI 探针驱动真实 Agent 构建并检查 ToolBridge 中是否有 Task。恢复探针覆盖 serializer→resolver→ceiling 的局部组合；没有构造完整 checkpoint/provider 恢复，不声称执行过该端到端用例。恢复结论还依赖 review.md 所列持久化生产调用链。

测试使用 Rust 1.93.1，显式 PATH 指向 rustup，`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUST_MIN_STACK=16777216`。复用共享 target；不清除其他并发任务使用的构建缓存。

## Reproduction

在 `agent/subagent/tests/mod.rs` 临时添加指向此 `probe.rs` 的 `#[path = "绝对路径"] mod delegation_audit_probe;`，运行：

```sh
cargo test --locked --lib -p shell audit_probe -- --test-threads=1
```

完成后删除该接线。探针中用了现有私有测试辅助函数，不能作为独立 Rust 可执行文件运行。

## Results

- `cargo test --locked --lib -p shell audit_probe -- --test-threads=1`：4 passed，0 failed/ignored，实际执行四个命名探针；最终日志 `/tmp/grow-delegation-audit-probes-verified.log`。
- native 路径别名：正式 ID 分别被 toggle/deny 拒绝，`./` 形式允许，两者对应同一文件。
- plugin 别名及目录：正式限定 ID 分别被 toggle/deny 拒绝，bare fallback 允许；toggle=false 的正式 ID 仍出现在目录。
- CLI-only：gate 允许，真实初始 build_agent_with_initial_overrides 构建的 ToolBridge 没有 Task。
- 恢复 recipe：原默认 ReadOnly 未写入可选 override，当前定义 All 被重算为新 ceiling。此项为局部链路验证，完整恢复限制见上文。

既有权限回归用同轮成功构建的 `target/debug/deps/shell-79d81af57ac7aa7e` 执行：`subagent_bash_permission_tests` 12 项、`session::subagent_capability::tests` 9 项、`session::actor::tool::dispatch::permit_tests` 4 项，全部通过，无忽略。最初一次 dispatch filter 匹配零项，核对 `--list` 后改为实际 `permit_tests`，不计零项结果。

首次临时测试发现 PluginId 导入路径错误；另有 fixture 未显式启用插件、CLI 构建缺少 LocalSet 和误用未激活资源域的 rebuild 入口，均修正为正常初始构建后完整重跑。不能以这些 fixture 失败作为产品 bug 证据。

临时源码接线已删除。生产实现和原有测试内容保持本轮审查前状态；仅新增审计文档、独立探针文件及 backlog 记录。归档前全量严格校验 18 项通过；归档后全量严格校验 17 项通过、归档校验 756 项通过，`git diff --check` 通过。已用 `skip_specs: true` 归档，不修改主规范。
