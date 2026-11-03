## Why
失败的冷load在注册actor后保留旧ChatState/配置，下一次显式load读了磁盘但仍复用旧失败快照，不能恢复已修复的依赖。

## What Changes
只退休本次load新建、尚处Suspended且未取得RestorePrepared的临时root actor；确认writer排空后下一次显式Retry重建。已有resident、Restoring和已释放的owner继续复用。

## Impact
Shell load和既有生命周期管理；不改持久checkpoint，不自动重试。
