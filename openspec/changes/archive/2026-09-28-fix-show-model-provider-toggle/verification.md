## 行为核对

- Settings 的 `show_model_provider` Bool 项可读取当前 cache 值。修复前 `action_for_bool` 缺该键，通用 Enter、Space 和鼠标值点击均得不到 Action；修复后共用 `Action::SetShowModelProvider`。
- 新增弹窗入口测试覆盖 Enter 从 Off 发出 On 动作、Space 从 On 发出 Off 动作。现有鼠标 Bool 行测试验证点击使用同一动作路径；现有 dispatch 测试验证即时 cache 更新、持久化 effect 与失败回滚。原有 compact 默认、Dashboard/child 显示契约及配置格式未改动。

## 验证记录

- `cargo test --locked -p pager --lib show_model_provider_row_toggles_from_settings -- --nocapture`：1 passed。
- `cargo test --locked -p pager --lib every_setting_has_action_for_bool_arm`：1 passed。
- `cargo test --locked -p pager --lib show_model_provider_updates_prompt_cache_and_rolls_back`：1 passed。
- `cargo test --locked -p pager --lib mouse_click_on_bool_row_dispatches_toggle`：1 passed。
- `rustfmt --edition 2024 --config skip_children=true --check crates/codegen/pager/src/views/settings_modal/state.rs crates/codegen/pager/src/views/settings_modal/tests.rs`：通过。
- `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：16 passed、0 failed。
- 本次测试首次构建生成 `target`；测试完成后执行 `cargo clean --target-dir target`，清理 47,148 个文件、12.4 GiB。

## 归档

- `openspec archive fix-show-model-provider-toggle --yes`：成功，将修改后的 client-surfaces Requirement 合入主规范。
- 归档后 `openspec validate --all --strict --no-interactive`：15 passed、0 failed。
- 归档后 `openspec validate --archived --no-interactive`：547 passed、0 failed。
- 归档后 `git diff --check`：通过；`target` 已清理，磁盘可用约 69 GiB。
