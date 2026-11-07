# Verification — 2026-11-06

已验收一个Session run loop缺失唤醒：idle状态下既有whole-work wait所依赖的人类交互解除时，旧循环没有监听已有registry watch；新分支调用既有idle仲裁。未声称复现或完全修复用户原始Goal自动接续故障。

实际实现只有 `run_loop.rs` 两处：select前订阅 `pending_interactions.subscribe_changes()`；新changed分支沿既有closed-writer处理、idle flush和 `arbitrate_idle_wake`。不新建轮询/task/registry，不改turn内等待。watch sender由actor持有，不存在session存活时的closed循环常烧；终止execution gate、controls→compaction→用户FIFO→notifications→Goal顺序由原arbiter负责。

Rust1.93.1、locked/offline、RUST_MIN_STACK=16777216、CARGO_BUILD_JOBS=2、单一Cargo窗口：

- 真实 `run_session` mailbox经GetActiveAgent barrier启动，随后test持久设置合法Goal/known-idwholewait；`RunCoordinationInquiry`外部cross-cwdPeer入口创建真实审批guard，gateway持有response模拟未答。先断言guard存在、planning wait成立、pending期间provider0请求且无GoalTurn。源取消inquiry后guard Drop发watch信号。
- 撤回本次两处fix时，主判别在“registry resolution must wake idle Goal continuation”超时失败，另外两控制通过；**1 failed / 2 passed、exit101**，`/tmp/grow-goal-interaction-old.log`。此前fixture两次前提失败（guard未注册先断言wait、Goal工具未注册导致runtime不可用）不算生产old-red，修正后才取得上述有效结果；其事实记录在此，不依赖覆盖后的临时日志保留全部历史。
- 应用fix后 `--lib goal_interaction_wake` **3 passed / 0 failed、exit0**，`/tmp/grow-goal-interaction-new.log`。控制涵盖StoppedGoal不重启，以及同session普通ask仍拥有busyforeground的前提反证。
- `--lib goal` **201 passed / 0 failed、exit0**；`--lib planning` **115 passed / 0 failed、exit0**，`/tmp/grow-goal-interaction-regress.log`。filters存在重叠，不汇总成独立场景数。

独立review `01a1b017-b310-7df0-a9c4-e0218b10d173` 亲核订阅、sender生命周期、busy/terminating仲裁、真实协调审批与old-red因果链，未发现blocking。父亲核实际diff/相邻owner和日志，更正coder“loop启动前seed”的不实描述：实际先启动mailbox、再seed、再准入inquiry；未把运行中的RegularTurn强设Idle，也未由test自行调用watch→arbiter。订阅仍早于命令处理，所以文档错误不推翻missingwake实验。

边界：这是已支持idle状态的module回归，seed已知inquiry id不证明模型自行发现该id；incomingapproval UiNotice不是模型pendingnotification。正常同sessionask未答时foreground仍busy。新测试没独立制造FIFO与Goal同时ready竞争，其优先序经共享仲裁静态核对及既有Goal/planning回归保留。审批取消取guard Drop路径；回答与超时共用registry signal，但本次不称三条实际入口全部重放。全链用户原始stall仍需可辨别证据。未改真实session/Goal/config，未提交或发布。
