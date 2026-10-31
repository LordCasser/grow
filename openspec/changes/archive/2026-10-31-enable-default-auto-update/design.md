## Decision

两个更新入口仅在 auto_update == Some(false) 时退出，与 Settings 和 leader 默认值一致。不写入配置、不覆盖显式关闭。

复用 event_loop 已有更新结果及 show_toast；完整模式显示目标版本和重启提示，欢迎页保留 Ctrl+U 操作，minimal 继续使用已有 notice。不新增状态、通知渠道或轮询。

## Verification

运行 update 现有 library 与 I/O 回归，验证版本策略、缓存与失败边界；核对完整和 minimal 的提示路径及显式关闭分支。对修改文件做格式检查；Pager 编译检查若被并行任务的共享构建锁阻塞则明确记录验证限制，执行 OpenSpec 严格校验后归档。
