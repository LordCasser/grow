## Validation
全仓 Rust 符号搜索及生产调用核对完成。实际运行 pager default_permission_change_is_future_session_only：1 passed，断言当前 Agent 权限不变，默认值改变且保存 effect 的 session_id 为 None。

## Limits
只验证默认设置与当前会话的隔离，不验证并发保存顺序，不执行 BestEffort 测试来冒充生产可达性。R8 尚未删除。唯一 Rust 改动为 setter 文档注释；无行为变化。linker 既有 __eh_frame 警告未影响结果。
