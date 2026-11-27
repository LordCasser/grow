# 验证记录

## 现场时间线

在 macOS 上用 Grow 2.6.19 的本地日志和 Timeline 按 child 身份对齐。父会话的 journal 已约 734 MiB。36 次工具调用自身的耗时中位数为 22 ms。单列 29 次全新委派，从父会话写入启动展示记录，到子会话接纳首个输入或产生失败结果，范围为 19.322–30.066 秒，中位数为 24.224 秒。观察到 30 秒发布回执超时。停顿期间其他会话仍有 Shell 事件，Pager 的界面事件在停顿后集中处理。

实时调用链为 `handle_session_notification_inner → enrich_from_timeline → load_timeline_by_id_at → validated_timeline`。每次发布都在界面事件处理中验证并折叠整份父历史。子会话已有获确认的 writer 和模型路由，首个 prompt 在等待启动发布回执。

## 对照方法

手动测试 `measure_live_subagent_spawn_reducer` 在同一未优化测试程序中先计时旧处理器的完整父历史读取（包含释放返回值），再从真实通知入口 `handle` 计时新处理器建立同一父会话下的 child view。计时包括反序列化和建立可打开的子页面。各样本独立启动进程，只读历史。模型请求、网络和首次 token 不在这两项计时内。

复测时将 `GROW_SPAWN_BENCH_PARENT` 设为本地父会话 ID，运行 `cargo test --locked -p pager --lib measure_live_subagent_spawn_reducer -- --ignored --nocapture --test-threads=1`。

第一轮用同一构建运行三个样本。期间机器也在编译，表中数值为单次实测。

| 父 journal | Domain 事件 | 旧处理器中的父历史读取 | 修复后的启动通知处理 |
| ---: | ---: | ---: | ---: |
| 8.00 MiB | 671 | 1.128 s | 2.35 ms |
| 31.59 MiB | 5,185 | 6.073 s | 1.09 ms |
| 63.13 MiB | 10,863 | 13.316 s | 75.13 ms |

用最终 Pager 测试二进制复测同一组数据。8.00、31.59、63.13 MiB 三份历史的旧读取分别耗时 1.163、6.003、7.976 秒，新处理器分别耗时 9.44、12.03、0.89 毫秒。该对照覆盖实时新建的通知处理；带继承正文的 `resume_from` 仍需验证来源并回放子历史。

## 边界检查

- 在父子磁盘资料与实时通知刻意不一致的夹具中，首次打开显示通知给出的任务、cwd 和 worktree，随后正常显示首个输入和模型正文。磁盘上的旧工具行没有进入这个新子会话。
- 有继承正文的委派仍按顺序回放。历史通知、省略新字段及父回放期间收到的启动沿用各自原有的 child replay 规则。重建通知使用 canonical Spawn，写入展示历史时省略传输字段。
- 控制发布、首个 prompt 与启动 ACK 的顺序有真实 Shell 回归覆盖，包括 ACK 失败、取消、丢弃与超时。恢复委派的父子校验仍先于准入；用占用唯一 blocking worker 的测试确认调度线程可运行，释放后可观察到原有的来源缺失错误。

## 运行记录

- Rust 1.93.1，`cargo test --locked --offline -p shell -p pager --lib subagent`。Shell 423 项通过，Pager 210 项通过，手动计时用例另行执行。
- 定向运行 `child_startup_waits_for_parent_publication`、`task_resume_inherits_completed_and_cancelled_transcripts_before_live_and_cold_view`、`task_resume_selection_reaches_provider_and_preserves_canonical_source`，3 项通过。
- 默认功能 CLI 构建通过。首次构建时 OpenHarmony SDK 的同名 `diff` 使 jemalloc configure 错误地跳过头文件生成。将系统工具目录置于 SDK 之前后重建通过。
- 最终 Pager 的 `cargo test --locked --offline -p pager --lib -- subagent descendant_recovery` 通过 224 项，两个手动 benchmark 默认忽略。
- 用刚构建的 `grow` 和本地模拟 provider 实际执行 `test_subagent_effort` 的全部三个手动集成用例，验证新建、恢复和拒绝无效模型选择。真实 PTY 用例 `parent_set_subagent_model_queues_then_child_applies_at_its_closed_step` 也通过，覆盖父工具派发、子 actor 启动、子页面和后续控制。
- `git diff --check` 通过，归档前全量严格校验 19 项通过。
- 归档合入两个主规范后，`openspec validate --all --strict --no-interactive` 通过 18 项，`openspec validate --archived --no-interactive` 通过 994 项。
