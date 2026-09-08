## Reproduction
旧 default_permission_earlier_failure_preserves_latest_choice 失败：AlwaysApprove -> Auto 后第一次保存失败，默认值变 ask，期望 auto。

## Validation
- pager app::root::dispatch::：881 passed。新回归覆盖最新选择、连续失败、先成功后失败，同时精确比较当前会话 permission_mode 保持不变。
- persist_default_permission_rejects_invalid_values_without_writing：1 passed；错误 SettingValue 类型及无效 canonical 均在配置写入前返回错误。
- 原默认/当前会话隔离和 reset 断言保留，reset 测试补入前一次保存成功事件。
- 生产检索确认旧 PersistPermissionMode 只剩 executor 匹配，无生产构造点；R8 已更新范围但未删除。

## Limits
不启动真实 ACP 或修改用户配置；正向写入复用已有 update_config，参数校验测试不触发文件 I/O。当前会话通知实现未改。进程退出/强制 abort 的持久性保证不在本次范围。既有 linker __eh_frame 警告未影响结果。磁盘剩余约 72 GiB。
