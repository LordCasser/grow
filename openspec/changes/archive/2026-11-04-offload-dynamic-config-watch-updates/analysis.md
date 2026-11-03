# SessionActor 启动中的项目发现 watcher

## 失败定位

`suspended_child_cold_restore_retains_birth_authority_end_to_end` 中 `authority.cold_load` 包住整个 `MvpAgent::load_session`，不是只包权限检查或 `restore_session_tree`。失败日志在新 root `SessionActor` 开始运行后记录 `fs_notify_config: None`，此后没有 `fs-notify: skipped/spawning`，且超时发生在 `load_session` 完成前。该结果不能证明出生权限恢复失败。

源码顺序给出直接阻塞边界：`session/actor/run_loop.rs::run_session` 在 `fs_notify_config` 日志后同步调用 `ProjectDiscoveryWatcher::start(cwd)`，而 `config/watcher.rs::ProjectDiscoveryWatcher::start` 同步创建 Debouncer、调用 OS `watch(project_root, NonRecursive)` 并挂接 `.grow` discovery dirs；只有这一步返回后 run loop 才继续初始化其余 worker、建立 command select。另一方面 `MvpAgent::spawn_and_register_session` 在 actor spawn 后发送 `InstallCoordinationBackend` 并等待 SessionActor ACK。若 native registration 阻塞，Actor 不会开始消费 command，load ACK 也无法推进。该调用与用户的 cold-load 日志吻合；目前原日志无法区分卡在 Debouncer 构造、root watch 还是 discovery-dir watch。

## 出生权限恢复边界

被测权限链仍应独立保留：`restore_session_tree` 从 canonical root Timeline 取得原 Spawn，在 adoption 前验证 `restored_birth_capability` 和 `birth_mcp_servers`；`run_shell_child_inner` 再使用同一个出生 capability 并与当前直接父 ceiling 相交。缺失/非法出生事实维持 fail-closed。未发现权限代码能解释当前日志中 SessionActor startup 尚未继续的停点；不得改权限值、当前父 ceiling 或 fixture timeout 来消除这次失败。

## 修复与回归

`run_session` 将 ProjectDiscoveryWatcher 的 start、native changes receiver、`refresh_new_dirs` 和最终 Drop 保持在同一个 `spawn_blocking` owner。该 worker 经传入 runtime Handle 等待 changes/cancellation；每次事件先由 blocking worker 刷新 native watches，再通过无界 channel 通知 LocalSet 异步 reload skills/workflows。LocalSet observer 同时消费事件和监视 worker；session cancellation 或 observer drop_guard（包含 TaskSlot abort）都会取消 worker 等待。仍在 OS 调用中的 registration 不可取消，observer 不等待该调用；worker 返回后在自身线程 drop watcher。SessionActor command loop不等待 watcher注册，仍可消费 `InstallCoordinationBackend` 并 ACK。

## Session runtime shutdown 回归

此前 helper 在 `run_session` 中读取 `Handle::current()`，而 `spawn_session_on_thread` 为每个会话另建 current-thread runtime；于是 `spawn_blocking` 实际归 session runtime。LocalSet cancellation 虽能及时结束 observer，Tokio `Runtime::drop` 仍要等待其 blocking pool 的 native start/Drop；该等待拖住 `SessionThread` 退出，导致 cold resume close 的既有 writer join deadline 超时。`/tmp/grow-260-effort-isolated.log` 中 `cold_resume_preserves_durable_effort_chain` 复现为 close 返回 `session is still shutting down; retry after its writer exits`。之后完整 core 的 Shell 结果仍有 5 项失败，其中 native teardown 失败包括 cold-resume 后 child writers 尚在退出、effort close、task resume 和 project-rules resume；出生 authority 的权限、MCP 与 cold-load 阶段均已完成。该证据指向 SessionThread/writer retirement 延迟，不支持放宽出生权限或增加等待期限。

Production `spawn_session_on_thread` 现捕获调用方 runtime Handle 写入 `ToolContext::watcher_runtime`；`run_session` 将该 handle 交给 ProjectDiscovery watcher helper。observer 仍归会话 LocalSet；native blocking owner、其 `Handle::block_on` 与 `spawn_blocking` 都归调用方 runtime。取消后 session runtime 和 writer 可先退出/确认；host worker仍保留 watcher，native 调用最终返回后才在该 worker drop。direct `spawn_session_actor` 路径的 `ToolContext` 默认无 override，继续使用当前 runtime。

`session_runtime_and_writer_ack_finish_before_host_watcher_owner` 建立一个独立 session current-thread runtime，阻塞 host worker 的 native start，发送/消费真实 `PersistenceMsg::FlushAndAck` fixture，再取消并等待 session runtime 完整 drop；断言这时 native owner 尚未 drop，释放 start 后 owner drop thread 与 start thread相同且不同于 session thread。测试中的 writer receiver 是受控 fixture，证明 ACK 与 session runtime shutdown 不等待 native 调用；它不替代真实 persistence writer 的 join，也不替代 `cold_resume_preserves_durable_effort_chain` production teardown 回归。两者均须在本轮代码构建后实际运行，当前没有将此修复记为已验证。

新增 `session/actor/tests/project_discovery_startup_tests.rs` 使用受控 blocking registration 验证：注册阻塞期间 command ACK 可推进；注册返回后 refresh 和最终 Drop 的线程 ID 均为 blocking worker、不是 LocalSet。该测试不运行文件系统负载，也不替代 macOS 原 watcher/MCP 失败矩阵和 birth-authority exact regression。

验证状态：本次源码/测试修改尚未编译或运行。待主构建完成后至少运行：

```sh
cargo test --locked -p shell --lib project_discovery_startup_tests -- --test-threads=1
cargo test --locked -p shell --lib suspended_child_cold_restore_retains_birth_authority_end_to_end -- --exact --test-threads=1 --nocapture
```

仍需由发布门禁运行 Pager MCP hot-reload 全组、Shell watcher 目录创建/重建全组及最终 18 crate 核心回归。单一 birth test 变绿不能关闭 watcher 阻塞项；watcher 修复也不能省略原出生权限正负例。
