## Why
普通 PersistSetting 每次独立 spawn，失败结果仅携带 key 和 rollback_value。用户连续修改同一设置时，较早失败会直接覆盖较新乐观选择；连续失败还可能恢复到从未保存成功的中间值。

## What Changes
建立普通设置写入的顺序与回滚归属，保持用户最新选择可见，失败回到最后确认状态。同一 key 的后续保存必须有确定顺序，不依赖磁盘锁偶然获得顺序。

## Capabilities
### Modified Capabilities
- client-surfaces: 连续设置保存的结果归属。

## Impact
Pager 普通 PersistSetting 调度和回滚；权限模式有独立通知策略，需要单独确认接入边界。不得只增加结果序号而忽略失败链或磁盘写入顺序。
