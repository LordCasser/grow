## Evidence
Effect::PersistSetting 原来独立 spawn；SettingPersisted 仅日志，失败无条件回滚。实际 dispatcher 回归证明较早失败把 None -> Minimal -> Fullscreen 的最新选择恢复为 None。

## Decision
AppView 持有按 key 的在途回滚基线及最多一个最新排队选择。首个排队选择的 rollback 表示在途请求成功后的基线；后续合并只替换目标值，保留该基线。在途失败则把原确认基线传给后续请求，UI 保持最新选择；没有后续才回滚。成功则派发后续请求。这样无需按值比较或 generation 来掩盖磁盘写入乱序。

统一 dispatch wrapper 记录递归深度，只有最外层登记普通 PersistSetting。reset、dashboard 等递归路径因此不会重复接纳。完成结果派发的后续 effect 再经同一接纳边界。

## Boundaries
PermissionModePersist 使用独立 effect/通知策略，不进入普通设置队列；未接纳 key 的结果保持既有处理。不同 key 独立推进，底层文件写锁仍负责文件读改写互斥。本次不增加进程退出、任务强制 abort 或崩溃后的持久化保证。
