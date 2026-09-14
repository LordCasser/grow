## 范围与方法

2026-09-14，用户授权补齐盲区并实施。同一工作区已有其他未提交改动，本 change 只处理通信展示、必要的收件保留/恢复和相关回归；没有提交 Git 或安装替换用户运行中的二进制。确定性映射、方向、UI 与测试任务交由三个 `gpt-5.6-luna / xhigh` 子 agent，主 agent 负责架构收敛、持久化/恢复、源码复核和统一执行验证。

已加载 UX skill、Atlas、Karpathy guidelines。证据以当前规范与真实源码为准；截图只用于现象识别，不视为协议。Atlas 未 materialize 的符号用逐段源码核对，没有将空结果解释为无调用。

## 行为证据

| 场景 | 验证入口与结论 |
| --- | --- |
| 父子工具真实开始 | `parent_child_tool_starts_keep_real_titles_and_raw_inputs` 调用真实 `send_tool_call_start`，覆盖三个工具的标题、目标、Other kind、完整原始输入与 interrupt；Pager tracker 对 typed input 和真实形状 raw result 继续验证完整展示。 |
| 发送状态与结果 | tracker 回归覆盖目标任务/Session、AskParent 来源任务区别、完整问题/回答、raw JSON、null、真实 coordinator 超时/ACK 不可用/取消错误文本，以及查询成功与 inquiry 业务失败分离。ACK 丢失用真实错误载荷验证展示，没有声称完成包级故障注入。 |
| 父消息接收与消费 | `parent_intervention_is_durable_attributed_and_has_explicit_timing` 验证原始 artifact、相同正文只改变 interrupt 的冲突拒绝、重复消息、消费后保留、orphan sweep、模型来源包装和关闭后的拒绝。 |
| UI 与模型隔离 | `parent_receipt_publication_is_transient_and_recovers_after_ui_disconnect` 验证真实 gateway notice 无 cursor eventId、有 transient 标记，消费后重发不追加 Timeline/模型/hook，断开 UI 不改变 durable receive。 |
| 保留集合 | ChatState 的 `parent_receipt_history_retains_consumed_payload_without_model_input` 验证与其他通知共享正文时的保留和不增加模型输入，同时拒绝父消息旧表示版本。既有通知清理与 owner 回归继续执行。 |
| 无 UI 缓存恢复 | Shell storage 的 `communication_receipt_replay_uses_timeline_without_updates_cache`、Pager child 的三项真实 ledger/artifact fixture 验证没有 updates 缓存仍恢复、先到 Notice 不跳过 ACP、重复打开不重复。 |
| 正文不可恢复 | actor/storage 回归验证 artifact 缺失时保留收件 ID 并明确降级；读取路径继续校验 hash/字节数。没有放宽 pending 的模型输入校验。 |
| 同/跨 Session 方向 | Shell inquiry 和 Pager coordination 覆盖父问子、子问父、peer、审批/失败、同 inquiry ID 不同来源、终态重放与主 turn 结束不终结 sideband。 |
| 子视图与重连 | parent receipt 回归覆盖普通 child、实际 nested spawn 路由、不抢焦点、live/replay 去重和 full/cursor reload。发现并修复 cursor tail 重复 Notice。 |
| 窄屏与可读文本 | 真实 block output 在 24/40/80 及极窄 8 列验证有界工具/目标/状态/中文预览，正文含图片路径仍为文本。Notice 在 GrowNight/GrowDay 下验证可读文本与行宽。 |
| 详情、复制与 Minimal | Pager 既有 Enter、双击、选择与新增 Notice copy 回归；Minimal 的真实 commit pipeline 验证运行中 receipt 只提交一次，问答仍等待自己的终态。full/cursor 及 cache-free 交错历史的提交边界另外在 ScrollbackState 回归中核验。 |
| 跨进程 | `scripts/test_local_coordination.py` 使用隔离 GROW_HOME、两个/多个独立 stdio 进程和 loopback 模型；补查真实 ask_session 开始标题、rawInput、peer 接收方向及重载身份，保留 FIFO/权限/取消/崩溃恢复等已有覆盖。 |

## 执行结果

构建统一使用 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`。磁盘紧张时仅清理旧的可再生测试二进制、已失效本地 crate 产物和本轮禁用的 incremental cache。

- `cargo check --locked -p chat-state -p tools -p shell -p pager`：通过初始类型检查；后续以最终 test/build 验证为准。
- `cargo test --locked --lib -p chat-state -p tools -p shell -p pager --no-run`：通过。macOS linker 报已有大型二进制 `__eh_frame` 警告，无编译错误。
- ChatState 全部 lib tests：484 passed、1 ignored。
- Tools coordinator 相关测试：41 passed。
- Shell coordination、notification_drain、notification_inbox、communication_receipt、local_ipc：73 passed。
- Pager 最终相关回归：631 passed、1 ignored。覆盖 tracker、ACP handler、subagent、收件/询问、Minimal frontier、Notice/Other renderer、详情与选择入口。
- `cargo build --locked -p cli --bin grow`：通过；`python3 scripts/test_local_coordination.py --binary target/debug/grow`：退出码 0，全部 10 组场景通过，不使用付费模型。
- `cargo test --locked --lib -p pager-minimal`：87 passed，包括实际 native commit 的父消息去重测试。相关 Rust 测试合计 1,316 passed、2 ignored。
- 精准 `rustfmt --check`、`git diff --check`、Python 语法和 Markdown 本地链接检查：通过。
- 独立 Luna xhigh 只读复核收件保留、child replay、cursor tail 和 Minimal frontier，未发现可证实 P1/P2。
- 全量 OpenSpec strict（归档前）：18/18 通过。
- `openspec archive improve-agent-communication-presentation --yes`：成功归档到 `2026-09-14-improve-agent-communication-presentation`；主规范新增 4 项要求、修改 1 项。
- 归档后 `openspec validate --all --strict --no-interactive`：17/17 通过；`openspec validate --archived --no-interactive`：333/333 通过。
- 归档后已修正 design 的相对源码链接，本 change 链接检查与最终 `git diff --check` 通过。

## 平台与边界

本机为 macOS。上述 TUI 验证是实际渲染/键盘事件/提交流水线的自动化回归，没有把它表述为人工终端截图验收；Windows/Linux 原生 TUI 未在本机运行，继续由平台 CI 覆盖。父消息 source version 2 和必须显式提供的 inquiry direction 不兼容旧内部表示；旧数据不会被猜测转换或静默改写。没有新增跨主 Session 任意发指令、已读协议、自动重发或消息中心。
