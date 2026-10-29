# MCP 初始化超时归因审计

## 对照方法

- 基线：独立、干净 worktree，HEAD `87e71b4dbef78ffceeabce807c15b02882806e2c`，聚合改动前。
- 当前：同一 HEAD 加工具/Thought 聚合工作区改动，codegen diff SHA-256 `b23d27616fdc31965c0e7e92ab689c7d9b06542be21a58f9b758766113420469`。
- Rust/Cargo 1.93.1，locked 依赖。分别构建后复制 test executable，直接执行以免后续 Cargo 调用再次编译干扰测试；当前二进制 SHA-256 `f7a02d8e6d7735a177679517bbc2842f05151ff67b4d406ba806c2e0ac728ff8`，基线 `70f33a78a5e7253986cb9c9506918793db725e7a0df1b9c83e6f92659acc99d5`。
- 两者运行 cwd 都是 `/Users/lordcasser/workspace/projects/grow`，均设置 `RUST_MIN_STACK=16777216`、`--test-threads=4`。每项沿原夹具使用隔离 GROW_HOME 子进程；没有修改期限、测试或运行时代码。

## 已取得结果

| 版本 / 条件 | 结果 | 原始日志 |
| --- | --- | --- |
| 初次聚合完整回归 | MCP 1 通过、11 失败；另有当时未修复的 7 个聚合回归失败 | `/tmp/grow-unify-pager-full-2.log` |
| 当前版本 MCP 定向；期间另有编译 | 1 通过、11 失败，99.52s；11 项名单与初次完全一致 | `/tmp/grow-mcp-current.log` |
| 当前版本单项运行 | main catalog 与 global/project lifecycle 各自通过；另一相同 main catalog 单项运行在 initialize 超时 | `/tmp/grow-mcp-current-exact.log`、`/tmp/grow-mcp-current-late-sample.log`、`/tmp/grow-mcp-sampled.log` |
| 基线 MCP 定向；无编译 | 12 通过、0 失败，57.26s | `/tmp/grow-mcp-baseline.log` |
| 基线完整 Pager；无编译 | 7601 通过、0 失败、16 ignored，66.59s；MCP 12 项全过 | `/tmp/grow-mcp-baseline-full.log` |
| 当前版本完整 Pager；无编译 | 7612 通过、0 失败、16 ignored，66.99s；MCP 12 项全过 | `/tmp/grow-mcp-current-idle-full.log` |
| 基线，4 个独立进程并发执行同一组 exact 测试 | 12 通过、0 失败，17.36s | `/tmp/grow-mcp-baseline-contention.log` |
| 当前，4 个独立进程并发 exact；期间恢复当前编译产物 | 12 通过、0 失败，17.66s | `/tmp/grow-mcp-current-contention.log` |
| 基线，4 个独立进程并发 exact；期间有编译 | 12 通过、0 失败，17.23s | `/tmp/grow-mcp-baseline-build-load-contention.log` |

完整套件均未排除 MCP 测试，ignored 数量为原有测试声明。并发 exact 实验绕过单个 libtest 进程内的 serial 锁，各用例仍沿原夹具创建隔离子进程。两版这轮都未出现失败，说明并发或编译不是已证实的充分触发条件；不能把前一次复现与编译重叠直接解释成编译必然导致超时。

## 归因结论与证据边界

1. **未发现本次聚合改动引入的回归。** 当前完整套件 7612 项全过，基线完整套件 7601 项全过；两者全部 MCP 用例均通过。当前同一源代码/测试产物曾出现 11 项超时，随后无代码修改全过，证明其表现有时序波动。
2. **慢启动路径在 HEAD 已存在，且不经过聚合代码。** 现场捕获的阻塞在本机配置/技能 watcher 同步注册，gateway 尚未获调度；不是已进入远端 MCP initialize 后超时。该启动结构相对 HEAD 完全未改。
3. **没有取得基线超时复现，因此不能把 11 项逐一记为“基线已证实失败”。** 证据支持“既有启动路径对本机文件监听延迟敏感”的判断，但不是全 11 次 OS 栈的逐一证明。原归档中的“没有证据证明基线也失败”仍然成立；本记录补充了前后对照结果，不篡改历史验证记录。
4. 没有扩 timeout、增加重试、跳过失败或修改产品代码。生产环境是否受相同 macOS 注册延迟影响、为何某次 FSEvents 注册明显变慢，仍需启动阶段计时/就绪边界的独立修复或进一步观测，不能从本次日志宣称已修复。


## 代码路径与失败现场

共同失败位于 `mcp_hot_reload.rs:517–524` 或 `mcp_hot_reload/lifecycle.rs:62–70`，都是 5 秒 ACP initialize 等待。MCP server connect、session/new 和热更新断言尚未执行；不能把它叫作远端 MCP server 握手失败。

`spawn.rs:254–269` 同步启动配置 watcher；`:291–293` 只把 gateway 排进 LocalSet；`:315–346` 又同步启动 skills watcher；`:347` 首次 yield。排队的 ACP initialize 此前没有执行机会。

当前失败的现场 `/tmp/grow-mcp-current.sample` 捕获：

```text
acp-agent-worker
  spawn_agent_thread_direct (spawn.rs:260 / :319)
    ConfigFileWatcher::start / SkillsFileWatcher::start
      AccessFilteredWatcher::watch
        notify::fsevent::FsEventWatcher::watch_inner
          FsEventWatcher::run
            std::sync::mpsc::Receiver::recv
notify-rs fsevents loop
  FSEventStreamStart
    register_with_server
      f2d_register_rpc
```

锁定依赖 `notify 8.2.0` 的 macOS backend 每次 watch 调用执行 `stop → append_path → run`；`run` 等待新线程在 FSEventStreamStart 后发送 runloop（本机 registry `src/fsevent.rs:308–314, 460–495`）。逐目录注册会累计同步等待。快照证明该次启动在本机文件监听注册路径等待，不能仅凭快照把所有 11 项的每次失败都精确定位到同一条 OS RPC，也不代表永久死锁。

`git diff --exit-code HEAD` 已确认 spawn.rs、完整 MCP hot-reload 测试夹具、shell config watcher/reloader、ACP initialize handler、transport 分发与 Cargo.lock 相对 HEAD 都没有变化。聚合代码不在这批测试的启动调用链上。

## 最初 11 项失败清单

全部前缀为 `acp::spawn::tests::mcp_hot_reload::`，当前定向复现的失败名单完全相同。两版最后的完整套件中，以下各项均通过。

- `in_process_session_reloads_global_and_project_mcp_catalog`
- `lifecycle::cold_session_load_uses_current_catalog_and_restores_project_watching`
- `lifecycle::global_and_project_catalog_lifecycle_tracks_current_endpoints`
- `lifecycle::policy_identity::auxiliary_policy_hot_refresh_disables_and_retimes_existing_connections`
- `lifecycle::policy_identity::duplicate_client_names_are_rejected_before_new_load_or_update_mutates_session`
- `lifecycle::policy_identity::pending_initialize_and_cold_load_use_latest_auxiliary_policy`
- `lifecycle::policy_identity::server_name_identity_survives_layer_conflicts_reload_and_cold_load`
- `lifecycle::repository_ancestor_mcp_changes_refresh_nested_sessions`
- `lifecycle::resident_session_load_with_empty_client_catalog_removes_client_server`
- `lifecycle::sse_tools_changed_refreshes_schema_with_auto_restart_disabled`
- `lifecycle::stale_blocked_initialize_cannot_restore_replaced_or_deleted_project_servers`

## 复现命令

构建使用 `PATH=/Users/lordcasser/.cargo/bin:$PATH RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib --no-run`，基线额外设置 `CARGO_TARGET_DIR=/Users/lordcasser/workspace/projects/grow/target` 复用依赖缓存。从各自构建日志提取 executable 后，复制为不同文件以避免后续构建覆盖。

```sh
# 两个 artifact 都从同一原工作区 cwd 运行。
RUST_MIN_STACK=16777216 /tmp/grow-mcp-baseline-artifact/pager acp::spawn::tests::mcp_hot_reload --test-threads=4
RUST_MIN_STACK=16777216 /tmp/grow-mcp-current-artifact/pager acp::spawn::tests::mcp_hot_reload --test-threads=4
RUST_MIN_STACK=16777216 /tmp/grow-mcp-baseline-artifact/pager --test-threads=4
RUST_MIN_STACK=16777216 /tmp/grow-mcp-current-artifact/pager --test-threads=4
```

补充并发启动实验按上述失败名单加关闭 project-watch 的控制用例，用 Python ThreadPoolExecutor(max_workers=4) 为每项运行 `artifact --exact <name> --nocapture`；没有修改原测试。临时脚本 `/tmp/grow-mcp-startup-contention.py`，每次完整输出及退出码保存在对应 contention.log。

## 流程与资源清理

- 本审计只新增 OpenSpec 过程记录及独立 backlog 条目，不改变运行时或测试契约（skip_specs）。
- 归档前 `openspec validate --all --strict --no-interactive`：17 passed，0 failed。
- 已在原工作区重新生成当前 Pager 测试产物，避免共享 target 留在基线版本；该构建通过（`/tmp/grow-mcp-restore-current-build.log`）。
- 归档后全量严格校验：16 passed，0 failed；archive 校验：712 passed，0 failed。
- 清理本次新建的 Pager 增量编译目录，释放 1.94 GiB；移除两份临时二进制副本约 962 MiB，保留原始文本日志及当前 target 测试产物。
- 基线 worktree 归档工具拒绝删除当前主检出（`The task's primary checkout cannot be removed.`）；保持干净基线检出，后续工程工作仍应使用原工作区 `/Users/lordcasser/workspace/projects/grow`。未手工绕过删除保护。
