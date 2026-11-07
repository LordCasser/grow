# SessionActor publication receipt 验证

本增量只交付父 SessionActor 的通知 publication receipt。child bootstrap consumer、真实 PTY 重验和整体产品验收由后续独立增量负责。本记录不将已有 v9 失败证据改为通过。

## 内部接口与确认边界

`SessionCommand::GrowSessionNotification` 新增：

```rust
respond_to: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
```

调用方创建 oneshot，随原通知发送 `Some(tx)`，并在自身启动期限／取消 select 中等待 `rx`。只有 `Ok(Ok(()))` 才允许后续依赖 publication 的动作；`Ok(Err(_))`、receipt drop、调用方取消／超时都视为发布未确认。停止等待时关闭接收端。命令不新增 deadline 或 cancellation 字段。

成功意味着：原父 actor 路径已完成 SubagentStart observe Hook（适用时）、原 eventId 所有者 stamping、通知持久确认，并收到原 `GatewaySender::forward_with_completion` 的成功 handler completion。父 actor 只同步完成入 gateway FIFO，不在自身循环中等待 gateway completion。

源码核对发现原无 preview 路径使用 buffered append ACK。因此父确认本增量的 `Some` 无 preview 分支使用已有 `NotificationSender::append_update_durably`，只写一次；不把原 buffered ACK 追述为 durable commit。active preview 继续使用原 `PreviewIndependent` 持久确认和预算／失败归属。`None` 保留原通知路径，既有 Shell command constructors 和完整 patterns 仅机械补 `None`。

watchdog 从 handler 开始计时，覆盖 Hook、persistence 和 gateway completion，最长 30 秒；这是本增量的明确选择，不宣称已有统一启动期限。它同时观察 `durable_ui_cancel` 和 receipt 接收端关闭。结束后内部 completion monitor 被 channel 关闭唤醒，退出并释放 preview permit。超时只说明 publication 未确认，不撤销或重写原在途持久操作；晚到 gateway completion 不能再产生成功 receipt。

Hook／持久化错误仍通过原 `TimelineWriteError` 返回并触发 run loop 的 fail-stop。serialization、preview budget、gateway error／关闭／completion ACK 丢失、取消／超时是 `publication unconfirmed: ...` 的 publication 失败，不变成 canonical Timeline 错误。persist-only 和不转发的 progress 不返回成功 publication receipt。

## 验证状态

定向命令统一使用：

```sh
env -u CARGO_TARGET_DIR -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u RUSTC -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 cargo +1.93.1 test -p shell --lib --locked --offline <filter>
```

`publication_receipt` 的四项确定性测试覆盖：

- canonical spawn 后通过真实 `handle_grow_session_notification` 入口处理 `SubagentSpawned`，观察 SubagentStart Hook 的 Triggered／Completed；hold durable append ACK 时 receipt 未完成且 gateway 无消息，放行 durable ACK 后 actor 返回，但 hold 原 gateway completion 时 receipt 仍未完成；只在 gateway ACK 后成功，持久与 live 的 eventId 相同且不发送第二份通知。
- active preview 使用原 `PreviewIndependent` ACK；预算耗尽、gateway receiver 关闭、completion ACK drop、gateway error、actor 取消、30 秒超时、receipt receiver 关闭均不得成功。gateway response sender 的 `closed()` 是 completion monitor 已退役的握手，重新取得全部 4096 credit 证明 permit 实际释放；晚到成功 ACK 被拒绝。
- 有／无 preview 的明确写失败和 ACK 丢失保留原 Timeline error，不 forward、不成功 receipt。
- durable append 在途到期时 receipt 返回“发布未确认”；原持久 owner 仍可完成，晚到 completion 不复活成功 receipt，不宣称回滚。

测试使用 oneshot 和 channel 的到达／关闭握手与 Tokio paused-time advance，不通过 sleep 推断顺序。新测试直接控制 `actor.notifications.persistence_tx`，避免 `create_test_actor` 默认自动 durable ACK bridge 使屏障测试失真。首轮编译成功但三项测试暴露该 fixture 自动 ACK 干扰；修正夹具后再验收，不作为旧行为的回归红证。随后正例核实默认 `AdmissionAckPersistence::Inject(None)` 不向 observation 输出 Timeline，因此最终正例复用现有 `create_test_actor_ex_with_physical_projection` 和 `acking_persistence_channel`，实际观察 Hook owner 的 Triggered／Completed，不改变生产 Hook 行为或伪造观察事件。源码自查另保留了预算失败的原同步标记顺序，停止过一次尚未完成的重跑，最终结果以最后源码对应日志为准。

最终源码验证结果：

| 检查 | Actual exit | 结果与日志 |
| --- | --- | --- |
| 上述固定 Cargo 前缀，filter `publication_receipt` | 0 | 4 passed／0 failed，`/tmp/grow-child-publication-green.log`；task `01a1b29f-8009-74b3-8b48-a9ebf395ec85` |
| 上述固定 Cargo 前缀，filter `grow_event_id_stamping_tests` | 0 | 18 passed／0 failed，`/tmp/grow-child-publication-regress.log`；task `01a1b2a0-9800-73f0-adab-4789321318e4`。包含 `None` 通知、eventId／wire、active preview 及 Goal-owned Spawned→GoalUpdated 原顺序 |
| `rustfmt +1.93.1 --edition 2024 --config skip_children=true` | 0 | 仅本增量 `actor/updates.rs` 和新测试文件；diff 核对无范围外格式改动 |
| scoped `git diff --check` | 0 | 已修改的 owned Rust 文件与机械 None 适配文件 |
| `openspec validate --all --strict --no-interactive` | 0 | 21 passed／0 failed，`/tmp/grow-child-publication-check.log` |

编译保留既有 `input_admission.rs:2528` irrefutable `if let` warning，未扩范围修正。未运行 CLI／workspace／PTY／native 21 或跨平台检查；bootstrap consumer 尚未接入，本增量的模块通过不能替代后续真实跨 actor 启动链验收。

## 修改范围与交接限制

实现位于 `shell/src/session/{commands.rs,actor/run_loop.rs,actor/updates.rs}`，相邻测试位于 `actor/tests/publication_receipt_tests.rs`。`agent/subagent` 与 `agent/mvp_agent` 仅为现有 command 增加 `respond_to: None`；不实现 bootstrap consumer，不修改 Pager、未知 epoch 过滤、wire schema、权限或生命周期政策。

工作区原有大量未提交变更保留；本增量不 commit、push、archive、部署或访问真实用户状态。实现和模块验证完成，状态为待父 review。最终两个 Cargo task 均 actual exit 0，早期失败／主动终止的任务及观察 monitor 已终态；没有留下 owned compiler、测试或 watch 进程。Coder Result 交接时释放本增量的 Cargo、源码与此验证文档写入窗口，供下一 bootstrap owner 使用。
