## Context and evidence

Grow 的 `event_loop::run` 在 `crates/codegen/pager/src/app/root/event_loop.rs` 启动 detached、每 100 ms poll 的输入线程。`app/mod.rs::restore_terminal_with` 排空 writer、输出包含 Kitty pop 的 teardown、等 10 ms crossterm event、关闭 raw mode。正常 quit 和首次 signal 归入该恢复路径；`set_panic_hook` 和第二次 signal 只做 best-effort teardown。`kitty_flags_pushed`/`take_kitty_flags_pushed` 是协商实际成功与一次性 pop 的标记。已有 `WriterThread::join` 与 Drop 可能无界阻塞，不能把 DA1 等待放在它们之前。

已归档 `client-surfaces` 只要求终端 keyboard restoration，尚未规定 pop 后输入屏障。上游本地 `grok-build` 的 `xai-grok-pager-render/src/terminal/pop_fence.rs`、`xai-grok-pager/src/app/{reader_thread,teardown_fence,terminal_restore}.rs` 和三个 PTY 场景验证可借鉴；Grow 需要保留自身 writer 顺序与 signal 路由。

## Decisions

1. **只在可证明独占输入时 fence。** 将 reader 的停止句柄从 `event_loop::run` 传到终端恢复所有者；启动前为空。关闭接收端/设置停止状态后以覆盖一次 poll 的有限期限等待 reader 退出。未确认退出时不从主线程读 TTY，也不调用可能与 reader 抢锁的 crossterm drain。测试分别证明 reader 已退出和未退出时的决策。
2. **写入顺序与 deadline。** 正常路径先完成已接受 writer 帧的收束，再执行现有 teardown 顺序。必须在 `take_kitty_flags_pushed` 前取得是否实际 pushed 的值；仅在 pop 已写出、writer 不会再写入、终端为 TTY 时发 `CSI c`。DA1 读写与结果分类纳入固定的有限上界（目标 1 秒）；超时、EOF、错误均继续关闭 raw mode。writer 收束不能把“有界退出”隐藏在 `join`/Drop 或 stderr 锁里：实施时提供有界确认/可安全放弃的 writer 停止路径，所有正常 teardown 均有独立的 stderr 锁等待上界，失败时不启动 fence 并尽力恢复终端。不得为了 fence 创造晚于 teardown 的 writer 帧。
3. **仅消费 DA1 协议边界。** 在 raw mode 下从同一个 TTY 输入源读取，以字节级有限状态解析完整 `CSI ? [0-9;]* c`（包含可接受的 7-bit/8-bit CSI 表示时需测试）；DA2、Kitty CSI-u、部分回复不算完成。消费 pop 之后直到回复之前的输入，包含无法便携重注入的 typeahead；此取舍在退出边界明确记录。不从任意背景输入伪造 DA1 成功，也不在缺乏独占 reader 时开始读。
4. **异常出口不等待。** 首个受控 SIGINT/quit 使用正常恢复；panic、第二次 signal、`init_terminal` 自身失败使用现有 best-effort 输出和 raw restore，不在可能重入的 hook 内等待 reader、writer 或 DA1。初始化成功但后续连接失败时虽未启动 reader（`ReaderJoin::Absent`），主线程仍独占输入；若 Kitty push 和 writer 收束成功，可走正常 fence。fence 对非 Unix/非 TTY/未 pushed 路径无查询副作用。

## Risks / trade-offs

- 终端不应答 DA1 时正常退出最多额外等待设定上界；不将沉默等同于失败退出。
- DA1 不含 request id，极迟到的旧回复无法被绝对区分；严格匹配和只在 pop 后启动降低误收概率。
- 退出时用户新输入会被 fence 消费；无法保证跨所有终端重注入，本 change 接受这一有界窗口，不改变正常运行输入。
- 若 writer 卡在不可取消的系统写入，不能同时保证输出完全有序和绝对恢复；记录降级结果、避免再读 stdin，不能默默采用无界 `join` 作为“有界 fence”的一部分。

## Verification and rollout

先以无 TTY 单测校验 parser、决策和调用顺序，再用可脚本化 PTY 验证晚到 release、沉默回复、无 flags；PTY 测试允许 opt-in `#[ignore]`，但实际需至少运行一次并记录。额外回归普通 quit、首次/再次 SIGINT、panic 快速路径和 writer 失败恢复。若运行环境没有 PTY，明确未验证，不以单测代替。完成后更新开发者说明，严格校验并归档；不改变历史会话。
