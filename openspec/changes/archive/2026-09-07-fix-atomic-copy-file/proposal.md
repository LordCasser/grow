## Why
复制文件共用 write_owner_only，但该函数先截断目标再设置权限和写内容。部分写入或权限错误可能破坏已有 /copy 文件和默认备份，需要在保留私有权限语义的前提下改为原子提交。

## What Changes
同目录私有临时文件完整写入、同步后 persist。提交前失败清理临时文件并保留原目标。成功的新内容始终为 Unix 0600，不继承旧文件宽松权限。

## Capabilities
### Modified Capabilities
- client-surfaces: 私有复制文件原子提交。

## Impact
pager-render 的复制文件 helper，覆盖显式路径与默认备份。tempfile 从开发依赖移到普通依赖，复用工作区已锁定版本。
