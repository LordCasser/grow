## Evidence
原实现真实 /bin/sh 回归失败：exit 0 left a live descendant。首次修复暴露空进程组 ESRCH 被错误当作清理失败，现只将 Unix ESRCH 视为已清理，保留其他错误。

最终 `cargo test --locked --offline -p config --lib managed_text --quiet`：25 passed，2.71s。含真实后代（忽略 TERM，KILL 前不会退出）在成功与非零状态下均无法延迟写出标记、无后代退出结果保留、注入清理错误、原超时/等待失败及事务测试。

使用低磁盘编译配置，target 152 MiB，可用76 GiB。仅隔离临时文件与测试进程，未运行真实 doctor 修复；Windows 未实机验证。全量规范严格校验通过，归档后再次检查。
