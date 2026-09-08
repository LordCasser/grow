## Design
复用 ProcessOps::teardown 与既有 TERM、50ms、KILL、1s reap 路径，在 Ok(Some(status)) 分支统一处理成功和失败；保留非零状态与清理错误。无需新增进程管理实体。

## Scope
生产 doctor 的 SSH shell -n 使用此入口。进程组外逃逸、attach 失败的现有直接子进程回退不在本次扩展范围。Unix ProcessGroup 没有 Drop；Windows job 的 Drop 不能替代跨平台显式结果处理。

## Verification
真实 /bin/sh 在退出前派生忽略 TERM 的延迟写文件后代，成功与非零退出后都不得写出标记。注入清理失败验证成功不能掩盖错误。原有超时及 wait 错误测试保持。
