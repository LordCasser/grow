## Checks
全仓 rg 对 save_config/save_config_locked/save_config_at 的引用结果核对完成。save_config 无实际仓内调用；save_config_at 仍由 update_config_at 和回归测试使用，不能删除。源码注释调整不改变任何 Rust 表达式，不重新运行编译。

## Limits
未执行删除，未证明仓外公开 API 无消费者。上一轮实际持久化测试为 57 passed，本次不将历史测试冒充重新执行。
