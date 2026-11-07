## Analysis — repair-goal-turn-continuation（诊断，第 2 轮收敛）

- 性质：只读调查，无行为变更（skip_specs 理由：不改契约；行为 delta 由父确认根因后另立）。
- 主要源码证据：
  - `run_loop.rs` L1449–1514（completion arm 固定 idle 次序）、L3253（notified 分支）；`turn/mod.rs` L699–714（handle_turn_end）；`turn/settlement.rs` L417–473（degradation plan）；`idle_arbitration.rs` L85–99；`goal.rs` L61–80 / L782–827 / L829–905。
  - F1 收敛证据：`turn/admission.rs` :316 / :469（false）与 :478（true）覆盖 durable ACK 发送；`session_setup.rs:85` `ensure_prefix_ready` 的 10s 超时只包 background join handle，超时后 sync fallback `build_user_message_prefix().await` 无总 deadline，不能据此断言整体不会 park（通用 prefix 故障面，仍无已证 Goal 特有 root）；`tasks_cancel.rs` AgentTask 将 ack 存入 `TURN_DURABLE_START_ACK`，drop 未 send 时 oneshot rx 得 `Err(RecvError)` → `start_goal_internal_turn` 直接返回，无占位。
  - F2 收敛证据：`planning.rs:1147/1253`（turn 内订阅唤醒）；`planning.rs:1064–1077`（Question/Permission 就绪语义：pending→Ok(false)，解除→Err→`unwrap_or(true)` 视为 ready）；`coordination.rs:861-925`（外部 primary session 跨工作区 inquiry（InquiryAuthority::Peer、cross-cwd，非子 Agent）经 request_coordination_approval 在目标 session registry 注册 Permission guard，父可 idle，`APPROVAL_TIMEOUT` 解除不 notify idle arbiter——候选链 C1）。
  - 既有测试：`idle_arbitration.rs` 4 个 idle/goal 仲裁测试；`notification_drain.rs` goal revision 隔离测试。
- 第 1 轮两处断言已修正：F1"goal_drive 永久占位"撤销；F2"无任何 waiter"限定为 idle 路径且可达性仅剩 C1 候选（未证实）。明确证据缺口：本 session 未创建 Goal；主 session timeline（01a1ac97…）抽样未见真实 continuation 失败事实。静态结论不等于用户 bug 已复现。
- `.openspec.yaml` 已改为仓库合法格式（`schema: spec-driven` / `skip_specs: true` / `created: 2026-11-06`，对照 `archive/2026-09-22-repair-pager-recovery-regressions` 等）。
- T-C1 fixture 前置已源码核实（未写测试文件，待父统一 sourcewrite+Cargo 窗口）：真实入口链 `SessionCommand::RunCoordinationInquiry` → `run_loop.rs:2980` → `enqueue_coordination_inquiry` → `handle_coordination_inquiry`（`InquiryAuthority::Peer` + cross-cwd 且非 delegated 才走审批分支）→ `request_coordination_approval` 注册 `PendingInteractionGuard`（Permission, id=inquiry_id）并等 gateway 应答，`tokio::time::timeout(APPROVAL_TIMEOUT=120s)` 超时后 guard drop、无 idle_arbiter notify。可复用 `truncation_recovery_tests.rs:1727-1805` 的 idle-goal-wait fixture 形态（durable Planning whole-work wait + `arbitrate_idle_wake` + `idle_arbiter.notified()` 观测）；判别断言 = guard 解除后 `idle_arbiter` 出现 notify permit 并最终产生 goal_continuation turn（旧代码 red）。
- 判别实验 T-C1 / T-F1（回归防护）与日志判别标准见 design.md，未执行（Cargo 属父 AC01 唯一窗口）。
