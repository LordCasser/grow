# 实施与验证记录

日期：2026-09-29。

## 离线能力边界

- `cli/src/main.rs` 在普通配置/agent 分支前直接分发 `replay`；`pager/src/replay_cmd.rs` 只调用只读 reader、展示投影和终端 API，不构造执行 session、provider、工具、Hook、scheduler 或 ACP transport。缺失来源与非法 speed 在接管终端前报错。
- 实际 `grow replay` PTY smoke 使用隔离 `GROW_HOME` 的有效空 session；观察到独立 alternate screen、无 composer 的状态/控制栏，输入 `/help`、Enter、粘贴均不触发执行。按 q 退出码为 0，源哈希不变，终端恢复 alternate screen 与鼠标模式。`--speed 0`/`NaN` 和缺失 session 直接报错。
- `cargo test --locked -p shell --lib session::storage::transcript::tests`：5/5 通过；replay 与 export 使用同一个只读来源、rewind/reconciliation 及 authority 校验。

## 播放与最终展示

- `cargo test --locked -p pager --lib replay_cmd::tests`：8/8 通过。有限正倍速、极端无法安全换算值拒绝；虚拟时钟暂停/调速连续；倒退时间不重排；Unicode grapheme 的渐进正文在多倍速和暂停后恰好收束；独立 notice 在揭示期间到达、后续依赖事件等待揭示边界；大批到期事件分帧交付且不丢内容；完成展示与完整 Markdown 投影在 1×/4× 及暂停后逐字一致。
- 代码审查确认模拟分片仅保存在播放器游标，不写回 updates/Timeline；独立 Grow 状态在揭示期间交付，工具结果不生成虚假流；没有逐字事件队列。离线投影强制保留已记录 thinking，不用读取时的 Local::now 显示历史日期，工具不使用播放时墙钟作为原执行耗时。来源时间缺失时状态栏注明 estimated timing。
- `cargo test --locked -p pager --lib app::root::dispatch::tests::transcript`：36/36 通过，原有交互 TUI transcript 与文件队列未退化；`cargo check --locked -p cli` 和 `git diff --check` 通过。
- `cargo test --locked -p pager --lib acp::tracker::tests -- --test-threads=4`：161/161 通过，涵盖共享展示 tracker 的历史文本、thinking、attempt 归属和工具终态。

## 验证边界

真实 PTY smoke 使用小型空来源，复杂消息/工具的语义 parity 由单元 fixture 与共享投影保证；未对 512 MiB 长会话或极慢终端做端到端压力测试。原始逐 token 时间本就未保存，界面明确标注 simulated streaming。

## 归档

- 归档前 `openspec validate --all --strict --no-interactive`：15/15 通过；`openspec archive replay-session-transcripts --yes` 已将 delta 合入 `client-surfaces` 主规范。
- 归档后全量规范校验 14/14 通过。`cargo clean` 清理本轮编译产物 58.9 GiB，磁盘可用量恢复至约 55 GiB。
- `openspec validate --archived --no-interactive` 首次检查只因本条“归档后校验”任务尚未勾选而报告 11/12；完成实际检查并勾选后复验结果见最终校验。
- 最终复验：`openspec validate --all --strict --no-interactive` 14/14、`openspec validate --archived --no-interactive` 553/553，均无失败；`git diff --check` 通过。
