# 验证

- 基线：`cargo test --locked -p pager --lib dashboard -- --test-threads=1`，669 passed、2 ignored。
- 新增 AppView 输入回归在旧代码失败：`attached_ctrl_x_configuration_prefix_reaches_current_agent`，失败于 attached root 未收到 command prefix；涵盖 idle/running、prompt/scrollback 和五个 continuation。
- 修复后 `cargo test --locked -p pager --lib ctrl_x -- --test-threads=1`：18 passed。包含 child、默认重复键停止/关闭、过期重启、键盘 Repeat 不确认、自定义 stop key 和配置 continuation。
- `attached_ctrl_x_m_opens_current_session_model_selector`：1 passed，从 AppView 按键到生产 dispatcher、实际 ArgPicker 及配置 model identity。
- Dashboard 回归：670 passed、2 ignored，含鼠标/动作退出后 prefix 退休；leader_key 回归：13 passed。
- rustfmt 对本次修改的 Pager 文件执行；git diff --check 通过。既有 Atomic::fetch_update 弃用和 macOS unwind table 链接警告保留。
- 这是第一个独立增量，只修 command prefix 路由及导引，不把停止/删除/关闭生命周期问题报告为已修复。尚未运行真实终端 PTY 旅程，后续增量完成后执行隔离环境整体验证；本轮 Tier 2 使用真实 AppView + dispatcher 等价入口。
- 归档前全量 strict：17 passed、0 failed；归档后全量 strict：16 passed、0 failed，archive strict：767 passed、0 failed。
