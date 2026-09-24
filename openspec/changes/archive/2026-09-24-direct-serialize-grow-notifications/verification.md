# 验证

- `openspec validate direct-serialize-grow-notifications --strict --no-interactive`：通过。
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test -p shell --lib actor_persisted_grow_lines_carry_event_id`：通过（1 项）。
- 同配置运行新增的 `buffered_and_direct_grow_forwarding_keep_the_same_wire_payload`：通过（1 项）；断言两条转发路径的通知 JSON 完全相同且保留预期协议字段。首次尝试中 `_meta` 被误断言为 `meta`，修正后通过。
- 初次测试编译被并行进行的 response projection 类型调整阻断；相关类型修复后重跑通过。
- `openspec validate --all --strict --no-interactive`：通过（17 项）。
