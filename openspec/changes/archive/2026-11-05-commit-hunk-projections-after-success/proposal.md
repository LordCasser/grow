## Why

WS-06b在真实HunkTrackerHandle公共API中确认：reject磁盘写失败后，rejected统计和turn索引已改变，file hunks仍在。后续WS-06a要新增stale冲突拒绝，也依赖相同失败边界，故先收敛动作提交顺序。

## What Changes

由hunk action owner在单项或一文件批动作成功后才提交统计、turn索引和affected ids。失败文件保持未提交；批次此前成功文件保留真实已发生结果，不添加全项目回滚。

## Capabilities

### Modified Capabilities

- client-surfaces：hunk动作成功/失败投影与磁盘结果一致。

## Impact

hunk-tracker actor actions及公共handle回归；不改变磁盘版本比较（WS-06a后续独立实现）、diff算法或Shell action协议。整体数据完整性里程碑先修提交owner，再实现冲突条件。
