## Why

Grow 在推入 Kitty keyboard flags 后退出时，现有 `restore_terminal_with` 会发出 pop，随后仅用 10 ms 的 crossterm event drain 清理输入。终端异步发送的 keyboard release 可晚于这个窗口，被退出后的 shell 当作按键。更关键的是 `root::event_loop::run` 的 stdin reader 是 detached；恢复线程和 reader 可能同时读同一 TTY，无法可靠划定回复边界。上游 grok-build 已通过 pop 后的 DA1 往返消除此类残留，但其 reader 生命周期不能原样复制到 Grow。

## What Changes

- 正常退出/连接失败后的终端恢复先停止并有界确认事件 reader 不再消费 stdin，再排空已接受的 writer 帧，最后输出终端 teardown。
- 仅确实推入过 Kitty flags 且具备独占 TTY 输入、输出可写的正常恢复，在 pop 后、关闭 raw mode 前发 DA1 查询并有界消费直到完整 DA1 回复；不把残留 release 交还 shell。
- reader 无法停止、输出失败、非 TTY、Windows、未启用 Kitty flags 时跳过 fence 并执行既有 best-effort 恢复。panic 与第二次 signal 的强制退出不等待 fence。
- 使用 parser/恢复顺序单测与 opt-in PTY 场景覆盖晚到 release、沉默终端和无 Kitty flags。

## Capabilities

### Modified Capabilities

- `client-surfaces`: 终端 Keyboard Enhancement 的退出恢复拥有明确的 reader/writer 所有权和有界回复屏障。

## Impact

- 生产入口：`crates/codegen/pager/src/app/{mod.rs,root/event_loop.rs,signal_handler.rs}`、`crates/codegen/pager/src/terminal/`；如 writer 生命周期不能有界收束，相关 `pager-render/src/render/draw.rs` 仅作安全前置修改。
- 测试：`pager` 单测和 `pager-pty-harness` 场景；开发者说明链接 `client-surfaces` 契约。
- 不改变启动时 Kitty 协商、普通按键解释、minimal scrollback、其他终端协议。上游 `pop_fence.rs` 和三项 PTY 用例只提供行为参考，不以其实现作为 Grow 的所有权模型。
- 与进行中的 `inventory-all-crate-features` 盘点及现有终端改动独立；实施前核对并保留工作树中的其他修改。
