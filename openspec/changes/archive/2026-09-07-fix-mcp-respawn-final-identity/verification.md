# Verification

## 源码交错证据

- actor/mcp.rs::respawn_stdio 先在状态锁内安装 arc_client，再 await arm_liveness_watcher，最后重新获取状态锁。
- McpClient::arm_liveness_watcher 内部 await state_kind；最后重新获取 McpState 锁也可等待，因此安装不是最后异步边界。
- McpState::update_configs_diff 从 owned_clients 删除被移除或替换的服务器；保留服务器维持原 Arc 和 episode。
- emit_current_tools_changed 根据名称取得当前 episode，不能自行识别调用方是不是旧恢复。
- 修改后的最后一次状态锁中先核对 owned_clients 的 Arc 身份，失配返回 Superseded；匹配后直到 emit 和 Ok 之间没有 await。

## 覆盖范围

现有 superseded_final_respawn_does_not_publish_or_unregister 回归覆盖首次/末次 Superseded 的恢复循环行为，包括不推送成功/失败或耗尽、不注销工具。它用 MockActions，不模拟真实 actor 在监听器初始化时发生配置变更。本次生产交错窗口通过源码与锁边界核对，未增加仅重复身份比较实现的测试，也不宣称做了真实子进程交错集成回归。

## 执行结果

`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib mcp_ --quiet`：126 通过、0 失败、0 忽略，仅已有 __eh_frame 链接警告。
