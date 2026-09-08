## Why
技能扩展以 Tokio timeout 包装同步文件系统发现，无法中断单次 poll 中的慢扫描。现有超时错误还被映射为空成功列表。需要明确执行隔离、并发上限与错误返回，避免只改变错误文案或不断积累后台扫描。

## What Changes
为技能扩展的重载建立有界阻塞执行边界；超时和 worker 失败返回错误。已完成配置保存不因重载失败被伪装成回滚。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能扩展重载的执行与失败边界。

## Impact
限 shell 技能扩展，不在本轮改所有 session/inspect/workflow 发现调用者。
