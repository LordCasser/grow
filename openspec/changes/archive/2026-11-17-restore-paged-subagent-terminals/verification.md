# Verification

- 用户给出的会话以只读方式核对。父 Timeline 在挂起前已提交 7 对 Spawned / Ended，终态为 5 Completed、2 Cancelled，均有关联的 child result。TreeSuspended 只包含根。使用隔离的 Grow home、会话副本和 loopback provider，通过已安装的 v2.6.15 执行带分页标记的 `session/load`，返回的 descendant ownership 选择 7 个子任务并保留这 7 个终态，加载期间没有单独派发子任务生命周期通知。这解释了旧 Pager 为何只凭合成 Spawned 形成运行状态。
- Rust 1.93.1 下执行 `cargo test --locked -p pager --lib app::acp_handler::tests::descendant_recovery -- --test-threads=4`，10 通过，1 项手工基准忽略。新增回归核对页外取消终态、记录耗时和用量、子视图 idle、父状态未误判为等待，以及重复恢复不增加正文。
- 对同一 Pager 测试产物执行 `subagent`、`views::tasks_pane::tests` 和 `history_window` 过滤，分别 211、53 和 10 通过。任务栏的取消入口以及主视图的子任务等待均基于 `SubagentInfo::is_running`。真实加载和测试夹具均未使用线上模型。
- `cargo build --locked -p cli --bin grow` 通过。用各自隔离的会话副本从真实 `grow --fullscreen --resume` 入口对比。安装版的 Pager 日志只有 7 次 Spawned 状态投影，终端出现历史条目的取消按钮。本次调试构建的 Pager 在同一加载中应用了 7 次 Spawned 和 7 次 Finished，后者为 5 Completed、2 Cancelled。测试传输使用本地 loopback 配置。
