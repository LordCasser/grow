# Verification

## Scope and implementation review

- Task 预检与 child runner 共用 typed rejection 文案；定义存在性先于 toggle 与父 filter 判定，保留实际父 Agent 身份和允许的完整 ID。
- toggle 由共享当前快照供 coordinator、既有 spawn context、AgentRebuildSpec 与新 Workflow Run 的目录捕获读取。已冻结 Run 不刷新。
- daemon 与 in-process Pager 的 watcher 更新和注册完成协调走同一内部 reload 方法；有效表先发布，再向已有主/普通子会话发目录刷新。非法类型不发布空默认表。
- 目录刷新复用 Step control 队列、AgentPreparation 和既有 Agent 激活流程。准备期间变更在领取控制前重新核对；重建不占用取消控制锁。事务已领取后到来的更新在后续边界处理。
- 同名 Agent 的目录快照参与实际变更判定，子会话 Task 说明使用过滤后的完整 ID。全关后重新开启重新构建整个工具桥，恢复 Task 及其依赖。
- 新建但尚未注册的会话在启动末尾核对构建快照，避免错过 reload fan-out。

## Regression coverage

- `parent_restricted_type_returns_parent_error_before_spawn_in_both_modes`：同步和后台 Task 都返回父策略原因，未派发。
- `validate_subagent_type_honors_parent_agent_filter` 及路径测试：显式 true 不绕过父名单；未知短名不被误报禁用，完整路径可解析，false 保留全局禁用原因。
- `subagent_toggle_reload_rechecks_an_existing_spawn_context`：已经构造的 spawn context 观察更新和删除。
- `subagent_toggle_reload_tracks_changes_deletion_and_invalid_values`：watcher diff、删除、错误类型、重新启用，以及未扩展 permission_mode 热更新。
- `subagent_toggle_reload_updates_admission_and_primary_child_catalogs`：当前 gate 与主/子会话 fan-out 共用快照，冻结 Workflow 子会话不接收刷新，非法配置无副作用。
- `subagent_validation_reports_the_immediate_child_profile`：子委派按直接父会话的真实 profile 归因。
- `subagent_toggle_reload_removes_and_restores_empty_catalog_at_step_boundary`：主/子会话均保持当前 Step 目录，在下一边界移除并恢复 Task，描述与实际可用类型一致。
- `subagent_toggle_reload_invalidates_prepared_agent_catalog`：准备完成后改 toggle，实际 Agent selection 队列重建候选并保留用户选择。
- `subagent_toggle_reload_idle_drains_and_preserves_frozen_workflow`：空闲刷新与冻结路由边界。
- `subagent_toggle_reload_uses_shared_internal_route`：Pager 路由使用同一个内部方法。

## Execution

使用仓库固定 Rust 1.93.1，`PATH=/Users/lordcasser/.cargo/bin:$PATH`，`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUST_MIN_STACK=16777216`。

- `cargo test --locked --lib -p shell -p pager -p tools -p agent subagent -- --test-threads=4`：通过，agent 15、pager 190、shell 368、tools 29，共 602 项，无失败/忽略。
- 对本次 Cargo 成功输出的 `tools-3171838a04b44b5b` 运行 `implementations::grow_build::task::tests --test-threads=4`：63 项通过，包含新增同步/后台父策略拒绝用例。
- 对同次输出的 `shell-898a6417642c3d3c` 运行 `session::actor::model_switch:: --test-threads=4`：34 项通过，包含既有选择覆盖、取消/Step 边界和本次 reload 用例。
- 对同次输出的 `agent-f78826ddcf59473b` 运行 `child_task_description_is_concise --test-threads=1`：1 项通过。
- 上述计数为各次执行数，存在重叠；运行日志核对了实际匹配数和结果，没有把零测试退出当作通过。
- `git diff --check`、修改 Rust 文件的 rustfmt 及 `openspec validate --all --strict --no-interactive` 通过（18 项）。

首次编译暴露新增测试的 Option/String、context Clone 和共享 map 类型适配遗漏，已修正后完整重跑。最初 PATH 命中 Homebrew rustc，最终结果均来自显式 pinned 工具链。构建禁用了新增 incremental 缓存；共享 target 仍被其他任务的 Cargo 使用，不清除其既有缓存和正在使用的产物。

已通过 `openspec archive fix-agent-authorization-and-toggle-reload --yes` 合入两项主规范。归档后全量严格校验 17 项通过；归档校验在完成归档任务勾选后 753 项通过。最终 `git diff --check` 通过。

## Personal definition and deployment boundary

已读回 `/Users/lordcasser/.grow/agents/vulnerability-analyst/vulnerability-analyst.md` 的 `subagents.allow`，包含原有 `reverse/reverse-engineering-expert`、`malware-analyst/malware-deep-analyst`，新增 `general-purpose`、`vulnerability-analyst/vulnerability-analyst`；正文及其余策略未改变。这是用户明确授权的个人配置修改，不是内置全局权限放宽。

本次验证不连接线上模型，也未通过真实终端执行 Agents 面板点击/文件系统 watcher 端到端用例；覆盖 watcher diff、宿主路由、执行 gate 与 actor 真实目录重建各层。源码修复不替换正在运行的旧可执行文件。个人定义重新载入需重新选择该 Agent 或新建会话；旧进程的 toggle 热更新修复需使用新构建。
