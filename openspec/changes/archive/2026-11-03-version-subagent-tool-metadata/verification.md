# 验证

基于 v2.5.5 的源代码确认 TOOL_META_VERSION 原本已经为 5，新增三种闭合 kind 后没有提升版本。接口变更前已形成 proposal/delta/design/tasks；执行与权限模型不变。

- `UPDATE_TOOL_META_SCHEMA=1 python3 scripts/run_exact_cargo_test.py --package tools --lib tool_taxonomy::tests::tool_meta_schema_is_up_to_date` 重生成真实 schema。
- 不设更新变量的同一 exact runner：实际 1 test passed，非 0 test。
- `subagent_observation_vocabulary_uses_version_six` exact runner：1 passed，使用真实 CanonicalToolMeta 构造和 JSON 序列化，三种 kind 均为版本 6，名称、kind 和 Read access 保真。
- 最终 `cargo test --locked -p tools --lib tool_taxonomy::tests:: -- --test-threads=4`：7 passed，包含闭合 schema、unknown kind 拒绝、往返、嵌套 metadata 与版本场景。
- 最终 `cargo build --release --locked -p cli --bin grow`：24.86s 完成，无 warning/error。这里是普通本地 release profile，不是官方 release-dist/十平台产物。
- 最终 `cargo check --locked -p cli --bin grow`：22.19s；`cargo check --locked --workspace --all-targets`：1m12s，均通过，无 warning/error。
- 最终 CLI 隔离 GROW_HOME 的 version/help smoke 通过；version 为 `grow 2.6.0 (bbd2c4dfa)`，Mach-O arm64，macOS codesign strict 校验 valid on disk / satisfies Designated Requirement。

本地 candidate 仍基于 dirty source，commit 字段是原 HEAD 的普通 Git 发现结果，不能当作候选源码已绑定该 immutable commit。整个发布候选的 MCP/watcher/cold-load 门禁失败仍保留，未执行正式发布。

最终归档收尾：全量 strict Totals: 17 passed, 0 failed (17 items)；archived strict Totals: 828 passed, 0 failed (828 items)；format/diff 校验通过。资料归档不解除已记录的发布阻塞。
