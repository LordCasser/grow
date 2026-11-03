## Incident

用户指定 root：`01a19a05-5281-7201-bae3-89f599512353`，工作区为 `solaris-agent`。用户确认退出方式是连续两次 Ctrl+D。

以下时间统一使用北京时间（UTC+8），日期为 2026-11-03，进程起始时间除外。

| 时刻 | 来源 | 事实 |
| --- | --- | --- |
| 11-02 18:15:52.657 | `~/.grow/memtrace/1793614522-86472.jsonl` 的 `kind=start` | 事故进程 pid 86472 已启动，version=2.5.5。 |
| 12:24:23.409 | `~/.grow/logs/unified.jsonl` | 同进程记录 `pager quit`。 |
| 12:24:28.098 | 同日志，root | `shell.cancel.processing`，`trigger=shutdown`，`cancel_subagents=true`。 |
| 12:24:28.284 | 同日志，五个 child | 同时收到 `shell.cancel.processing`。 |
| 12:24:28.363–28.635 | 各 child Timeline | 原 Turn ended，outcome=cancelled；terminal source 标签为 `user_cancellation`。 |
| 12:24:28.600 | parent Timeline seq 15431 | 原 Turn ended，source=host，`cancellation_category=mid_turn_abort`，`details.trigger=shutdown`。 |
| 12:32:14.096 | 同日志，pid 2702 | 新进程开始 `session.load.start`。 |
| 12:32:16.461–17.419 | parent Timeline | 补记五条 `Subagent::Ended`，outcome=cancelled，error=`Subagent turn was cancelled: aborted mid-turn`。 |
| 12:33:40.304–40.914 | parent Timeline seq 15466–15473 | 后续退出提交 SuspendRequested、CommandsSettled、checkpoint、TreeSuspended；树只剩 root。 |
| 12:33:50.783–50.890 | parent Timeline | 后续恢复依次提交 RestorePrepared、Resumed、ExecutionReleased。 |

这次 12:24 退出之前没有对应的新挂起 operation/checkpoint。child 的取消发生在 shutdown；12:32 root resume 核对并补交已有结果。不能把 parent `Ended` 的补记时刻当作 child 实际停止时刻。`user_cancellation` 是终态来源枚举标签，不能单凭该标签推断用户手动取消五个 child；parent 的 host/shutdown 提供了更具体的同期证据。

## Child linkage

相关持久记录位于 `~/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fsolaris-agent/<session-id>/timeline.jsonl`。仅保存身份和事件坐标，不复制任务正文。

| Child session ID | Parent Spawn seq | Child Turn cancelled（北京时间） |
| --- | --- | --- |
| `01a19f1e-b1d5-72a1-b9ba-204b5467c622` | 13554 | 12:24:28.458 |
| `01a19f7c-8dbb-7a40-a0e0-8ee35eb79533` | 15029 | 12:24:28.635 |
| `01a19f7d-6300-7043-b6ba-135b8eb5b13b` | 15069 | 12:24:28.363 |
| `01a19f8d-0cb1-7773-b00a-722eaa5c9be9` | 15247 | 12:24:28.436 |
| `01a19f8e-983b-7111-9d89-3c11c30e2727` | 15345 | 12:24:28.471 |

## Current implementation and attribution

后续追查已定位 notification Turn 将 parent 标为 viewer、旧退出 guard 因此可能直接 Quit 的具体机制及回归，见 [补充调查](../2026-11-03-cover-notification-turn-exit-suspension/analysis.md)。本记录下文的“无法断言具体 UI 分支”仅指没有旧进程瞬态内存快照；不能把进程启动较早作为问题的根因解释。本文 seq 均为外层 journal seq，补充调查同时列出关键 domain seq。

`pager/src/app/root/dispatch/router.rs::begin_exit_suspension` 统一发出 `SuspendForExit`，`dispatch/task_result.rs` 只在匹配 ACK 成功后产生 Quit；失败保留 Unconfirmed。`shell/src/agent/mvp_agent/runtime_handoff.rs` 按 runtime scope 挂起并等待 writer 退役，child 恢复继续使用原 checkpoint；异常退出收尾在 `agent/subagent/mod.rs::reconcile_orphaned_subagents_with_backend` 验证持久结果后补记 parent terminal。

退出范围修复已有独立归档：`2026-11-02-scope-exit-suspension-to-runtime-owners`；其本地 verification 文件 mtime 为 11-02 19:49，晚于事故进程启动。当前安装程序和 `target/release/grow` 的 SHA-256 相同：`a7db05f60a0c6c9c8973afb275d19eca225a8efaa19f9c5c9929c62809a24423`。两者文件 mtime 分别为 11-03 09:22、07:12；它们不能反推昨晚已经运行的进程装载了哪些工作树修复。

可以确认旧进程的 Ctrl+D 退出没有完成本次树挂起，随后 shutdown 取消了任务。进程起始与修复时间支持“长驻旧进程尚未装载后续修复”的解释；旧进程缺少构建内容摘要和退出分支日志，无法据此断言具体是哪一个 UI 分支绕过了屏障。当前安装程序的行为另以 PTY 验证，不将源码检查当作旧二进制的证明。

同期三条 child attempt 还记录了关闭 Timeline 后结算失败的日志；当前已有 cancellation settlement barrier 的契约与实现，本次不改写旧数据或重做该独立修复。

## Recovery boundary

五个 child 已有 cancelled 终态，没有可消费的本次挂起 checkpoint。普通 root resume 保留终态符合现有契约；继续工作应沿已有显式 Task resume，继承原 child transcript，创建新的执行 epoch。该操作不等于原树自动恢复，不能通过删除终态或重用已消费旧 checkpoint 伪造。本次未对用户真实会话发起执行或修改其历史。
