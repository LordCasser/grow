## Reproduction
旧 update_removes_explicitly_cleared_optional_fields 失败：screen_mode 设为 None 后磁盘旧键仍存在。

## Validation
shell util::config::：232 passed。真实临时文件回归断言 screen_mode、cancel_subagents_on_turn_cancel、models.default 和嵌套 contextual_hints.undo 被删除，同时保留顶层及嵌套未知字段。既有路径引用、未知字段、类型错误及权限测试同时通过。

## Limits
使用实际 update_config_at 闭包路径；未经过 campaign dismiss 或 UI 全链路。显式整份 save_config 不推断清空意图。当前共享锁/双读取行为保留，不新增跨进程并发保证。macOS linker 既有 __eh_frame 警告未影响结果，未修改真实用户配置。
