# 验证记录

## 原因与实现边界

后端已有 `[subagents].permission_mode`，`RequestPermissionMode` 支持 ask / auto / always-approve，默认 Auto；`Config::resolve_subagents` 在启动时投影，child StartupHints 和 coordinator 沿该配置工作。Settings 原本仅注册 `[ui].permission_mode`，因此用户无法在菜单配置子 Agent 权限。

新增独立 `subagents.permission_mode` 项，Agent 类别、Minimal 可见、无选择预览、标注重启生效；快照、typed action、reset、effect 和 rollback 都使用独立字段，沿现有顺序保存协调器。Shell 仅在共享写锁下定向更改原始 TOML 的 permission_mode，复用原子写入，不物化 SubagentsConfig 的其他默认值。

对抗性审查核对全部完整 snapshot 构造点、主/子 mode 分离、无会话权限通知、Auto 的独立 gate 以及启动生命周期。审查纠正 Auto 文案，避免把可选 `request_only` 分类配置误说成必带 primary context。

## 验证过程

首次编译发现旧 AppView 测试夹具需补新字段，已补。首轮 Pager 子 Agent permission 相关 20 项通过；Shell 新夹具曾错误地把 TOML Value 的 inline display 当作完整 document 解析，已改为直接 typed 反序列化。较广 Settings 库回归识别三处需登记新 key 的既有清单/夹具，已补齐；最终结果另记如下。

## 独立基线缺口

通过 `git show HEAD:<path>` 比较确认：v2.4.2 的 catalog 已注册 show_model_provider，但 integration 的 ALL_SETTINGS_EXERCISED 没有登记它，也没有该 key 的键鼠用例。该问题在本次新增子 Agent 设置之前已存在，已登记 backlog；保留原断言，不混入本次实现。

## 最终结果

- `cargo test --locked -p pager -p shell --lib subagent_permission -- --test-threads=4`：Pager 20 passed、Shell 5 passed，0 failed、0 ignored。覆盖独立 current value、三模式 catalog、重置 Auto、打开 modal 的 snapshot 刷新、顺序保存与确认基线回滚、失败后再次保存，以及四项 Shell 文件保存/加载边界测试。
- 同次 Cargo 构建输出定位的 Pager libtest 产物 `target/debug/deps/pager-f67dd35bbcfb7b6c settings --test-threads=4`：331 passed、0 failed、1 ignored。较广组覆盖注册默认、整表 reset/rollback 分支与 modal 行清单；忽略项不计作通过。
- `cargo test --locked -p pager --test settings_e2e`：225 passed、4 failed。新增子 Agent 键盘三模式、鼠标及搜索用例全部通过，枚举成员清单已通过。四个失败均为 show_model_provider 在原有 integration 中完全未登记导致：every_registered_setting_is_exercised、registry_kind_membership_through_pr_14、defaults_round_trip_through_registry、settings_value_payload_matches_kind。`git show HEAD:crates/codegen/pager/tests/settings_e2e.rs` 确认该 key 出现次数为 0，而 HEAD catalog 已注册该项；不将整体集成组宣称为通过，不删除或跳过这些断言。
- 指定变更 Rust 文件 rustfmt 检查、`git diff --check` 与归档前 OpenSpec strict 17 项全部通过。

验证使用临时配置目录，未修改用户实际 permission mode。此次仅修改源码与规范，未构建安装或发布新的 Grow 版本。

归档后 strict 校验：Totals: 16 passed, 0 failed (16 items)；Totals: 661 passed, 0 failed (661 items)。

验证后执行 `cargo clean --profile dev -p pager -p shell`，清理相关产物 5,137 个文件、20.3 GiB；发行包缓存未纳入本次清理。

后续用户授权补齐上述四项集成清单遗漏，完整 Settings integration 已在 2.4.3 候选中达到 231 passed、0 failed、0 ignored；见 [后续测试维护验证](../2026-10-20-complete-settings-preference-test-inventory/verification.md)。发布跟踪见 [v2.4.3 准备记录](../2026-10-20-prepare-release-v2-4-3/verification.md)。
