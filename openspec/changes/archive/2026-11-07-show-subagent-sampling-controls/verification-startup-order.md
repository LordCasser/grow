# Child bootstrap publication 顺序验证

本增量消费已经独立 review accepted 的 SessionActor publication receipt，不修改其实现。真实 CLI v9 的旧红仍保留在 `/tmp/grow-subagent-control-product-evidence-v9/cli-diagnostics.log`；本记录只验收 bootstrap 模块与其真实 native 调用边界，最终 freshbuild CLI／原 exact PTY 由父架构负责。

## 实现边界

`agent/subagent/handle_request.rs` 仅在 `restore.is_none()` 的新 Spawned 路径（含派生 resume）等待 publication，最长 30 秒，并优先观察原 `cancel_token`。只有 `Ok(Ok(()))` 才继续 child 的 `PublishControlState` 与首个 `QueuePrompt`。取消不设置 `launch_error`，保留既有 Cancelled outcome；publication error、receipt drop、timeout 沿原 launch error、child result、parent terminal 和 actor 退役路径收尾。checkpoint restore 保留原快照／恢复路径，不新增 publication gate。

`agent/subagent/mod.rs` 的 private `publish_subagent_spawned` 向原 parent mailbox 请求 `Some(respond_to)`；绑定的 mailbox 关闭时直接失败，不绕过父 owner。只有原本明确 `parent_cmd_tx=None` 时沿原 eventId 构造和 direct gateway fallback 请求 `forward_with_completion`，同样受调用方的 30 秒／取消边界约束。每次只发送一份 Spawned，不重试、不代写 parent persistence。旧 `emit_subagent_notification` 保留 `None`，Finished／reconciliation 的行为未改。

未修改 SessionActor publication、Pager、PTY fixture、shared harness、manifest、control admission、Goal／Workflow 政策。工作区原有大量 dirty 变更保留。

## 确定性证据

在现有 `agent/mvp_agent/tests/session_model_tests.rs` native fixture 上增加独立 wrapper `child_startup_waits_for_parent_publication`，原 native wrapper 的 21 cases 保留，另跑 5 startup cases，不把原 21 改称 26。

五个 startup cases 均通过真实 SubagentEvent → coordinator → Shell bootstrap → child actor／persistence：

1. parent command proxy 握住真实 Spawned 命令时，child actor 已注册；通过 child Timeline owner roundtrip 确认没有 Turn，同时 gateway 没有 Spawned／child snapshot，provider 调用为零。
2. 将同一通知交给真实父 actor（仅替换测试用 receipt sender），等待真实 persistence 与 gateway publication ACK；consumer 的原始 receipt 仍被握住时，只有一个 Spawned，仍无 child Turn、snapshot 或 provider。
3. release case 完成原始 receipt 后，观察同一 `controlled-child` 的严格顺序：Spawned → 三条 ControlState snapshot → 首 provider。其余四个 case 分别返回 publication error、丢弃 receipt、取消 token、paused-time 推进 31 秒触发 30 秒 timeout，均不得启动 provider 或发 child snapshot。
4. cancellation outcome 为 Cancelled，failure／drop／timeout 为 launch failure；取消／timeout 通过 held sender 的 `closed()` 确认 consumer 已退役接收端，late success 发送失败。
5. 每个 case 的真实 child actor mailbox 已关闭、active child route 已移除；child storage 含且仅含一个 SubagentResult，父 Timeline 含一个 Spawned 和引用 child result 的 terminal，`finalization_pending=false`。不以 sleep 推断顺序。

相邻 `subagent/tests/startup_publication_tests.rs` 补充绑定 parent mailbox 关闭不触发 gateway fallback，以及无 parent 时 direct gateway 的 completion hold／success／drop、不重复发送和原 eventId stamping。该 helper 检查只补 fallback 边界，不能代替上述真实 bootstrap 五场景。

## 实际验证

所有 Cargo 命令使用：

```sh
env -u CARGO_TARGET_DIR -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u RUSTC -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 cargo +1.93.1 test -p shell --lib --locked --offline <filter>
```

首轮 test 编译因新增测试误用 `TimelineEventKind::TurnStarted` 返回 actual exit 101，已按真实 `TimelineEventKind::Turn` 修正；这是夹具编译修正，不作为产品旧红。随后 filter `startup` actual exit 0、12 passed（含新五场景 wrapper 和两个 fallback tests），task `01a1b2a8-aa82-77a3-8769-e878d25d5c42`。之后补强首次持有 parent command 时的断言与持久 terminal 检查，并明确保留 restore 分支行为；最终源码的命令结果以下表为准。

最终串行命令 task `01a1b2aa-e052-71a0-8c8a-59f399b42006` 由执行者启动后，执行者因运行时 `missing created_at` 响应解码错误终止；父接管了现有命令观察，没有重叠启动 Cargo。命令实际执行 291.45 秒、exit 0，最终源码结果如下：

| Filter / 检查 | 结果 | 证据 |
| --- | --- | --- |
| `native_subagent_model_tool_controls_the_child_without_waiting_for_its_step` | wrapper 1 passed，内部原21场景均通过 | `/tmp/grow-child-startup-regress.log:23` |
| `agent::subagent::` | 252 passed / 0 failed | `/tmp/grow-child-startup-regress.log:296` |
| `child_startup_waits_for_parent_publication` | wrapper 1 passed，内部新5场景均通过 | `/tmp/grow-child-startup-green.log:21` |
| 新增 fallback 测试 rustfmt check | exit 0 | `/tmp/grow-child-startup-final-fmt.log` |
| owned existing files `git diff --check` | exit 0 | 父在最终命令退出后执行 |
| OpenSpec strict | 21 passed / 0 failed，exit 0 | `/tmp/grow-child-startup-check.log` |

父另冻结最终四个源码文件 SHA-256：`/tmp/grow-child-startup-final-source.sha256`。原编译 warning 保留；不因执行者终止而否认已实际完成的验证，也不将缺少最终消息当验收通过。

仅对本增量新 helper／gate／startup 测试块应用 `rustfmt +1.93.1 --edition 2024 --config skip_children=true` 的格式结果；相邻新增测试文件完整格式化。保留既有 `input_admission.rs:2528` irrefutable `if let` warning，未扩范围修正。未跑 workspace full、CLI、PTY 或跨平台检查；上游已验收的 4 publication receipt／18 None stamping 不重复运行。

## 交接

父已核对最终 gate/helper、真实 command proxy 的两个持有阶段、同 child 的三域 snapshot→provider 顺序、五种终态及持久结果断言，并在现有命令实际 exit 0 后完成差异和格式检查，接受本模块增量。执行者因运行时响应解码错误未形成正常 Coder Result；父确认其模型执行已失败终态、原 Cargo/测试进程均退役，接管并收齐验证。没有 commit、push、archive 或修改真实用户配置。该模块通过不能替代最终真实 CLI 产品验收，父后续 freshbuild 并重跑原 exact PTY。
