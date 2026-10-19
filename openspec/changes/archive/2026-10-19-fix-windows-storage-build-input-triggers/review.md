# 对抗性审查

- 删除 `.cargo/**` 或 `Cargo.toml` 任一触发项时，`test_windows_filter_covers_direct_build_inputs_for_both_source_triggers` 会因缺少必需路径失败。
- 只改 PR 或只改 push 的路径列表时，同一测试会因两组触发器不一致失败。
- 发布路径的反向检查沿用 `release_contract_errors`：删除 Windows gate 的 `publish.needs`、移除显式成功条件、改用未经验证的 SHA，或删除 reusable workflow 调用，均会使测试失败。
- `workflow_call` 保持顶层可复用入口并要求调用方传入 commit；release workflow 的门禁不依赖 PR/main 的路径筛选。检查确认 publish 同时依赖 Windows gate 且要求 `success`。
- 触发器仍以原来的窄文件集合为准，只加入会影响构建的 workspace 配置。静态测试不能证明 GitHub 服务端的 path-filter 运行表现，也不覆盖 hosted Windows 编译执行。
