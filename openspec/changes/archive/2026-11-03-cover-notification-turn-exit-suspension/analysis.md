## Root cause

双 Ctrl+D 的第二次按键正常确认 Quit；错误发生在确认之后的退出分派。旧 `begin_exit_suspension` 用 `session_id.is_some() && !attached_as_viewer` 筛选页面。没有这样的页面时直接返回 Quit，跳过 `SuspendForExit`。

`attached_as_viewer` 实际是逐回合的消息归属标志。真实 ACP handler 在收到不同 prompt 的 live delta 时，根据 `!agent.is_self_originated_prompt(notif_pid)` 更新它。后台完成通知触发的宿主 prompt 不是客户端提交的，普通单客户端的本地会话也会被标为 viewer。这个标志不能证明本地 runtime 没有 child、foreground 或 writer。

于是存在完整的错误链：

1. 本地用户发起工作并启动后台 child。
2. 一个已完成 child 的通知使 parent 自动进入 internal Turn；首个 live chunk 将 parent 的 viewer 标志设为 true。
3. 用户只按两次 Ctrl+D。旧 guard 看不到 non-viewer 页面，直接 Quit。
4. Pager 的 `AgentShutdownGuard` 取消 worker；`stop_watchers_and_flush` 调用 `flush_all_sessions`，向仍在运行的 SessionActor 发送 Shutdown。
5. `begin_graceful_shutdown` 取消当前 Turn 并 drain/cancel 子任务。没有此前确认的 checkpoint，child 因而变成 cancelled。

展示投影被错误用于 runtime 生命周期判断，是这条链的根因。进程较旧只解释为什么后来移除 guard 的修复未进入该进程，不是根因本身。

## Incident correlation

会话 `01a19a05-5281-7201-bae3-89f599512353` 的 parent journal seq 15382 / domain seq 11747，在北京时间 2026-11-03 12:22:44.247 启动 Turn `12458073081155420161`，identity 为 `origin=subagent_completed, turn_kind=internal`，prompt 为 `notifications-01a19fa5-2ab6-7a72-a2fc-82f909e7e9e4`。

12:24:28.098 的 `shell.cancel.processing` 指向同一个 notification prompt，trigger=shutdown；parent journal seq 15431 / domain seq 11783 随后关闭同一 Turn。12:24:23.409 已记录 `pager quit`；中间没有本次挂起意图或 checkpoint。五个 child 在 12:24:28 同期取消，12:32 load 后才补交 parent Ended。

旧发布二进制缓存报告 `grow 2.5.5 (5998735)`；该提交源码保留上述 guard。退出前旧 UI 的 viewer 值及页面集合没有持久化，不能声称直接读取了其瞬时内存。事故的通知 Turn、无挂起的直接退出、shutdown 结果与可重复的代码缺陷一致；回归用于证明这条触发链，而非补造旧内存快照。

## Correction to the first investigation

前次普通递归树 PTY 覆盖原用户 Turn 等待 child/grandchild，没有覆盖通知回合导致的 viewer 切换，因此不能用它排除此缺陷。它仍证明当前安装版在该测试覆盖路径上的真实挂起/恢复；本次新增生产 ACP → 键盘 → 退出分派的对应触发回归。

前次记录所用 seq 是外层 journal seq；域 Timeline 另有 domain seq。本节为关键事件同时给出两者，避免混淆展示事实与域事实的坐标。
