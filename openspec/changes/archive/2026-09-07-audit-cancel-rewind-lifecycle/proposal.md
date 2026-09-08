## Why
持续审计 cancel rewind 的草稿/附件恢复与晚到响应；发现队列恢复注释与实际服务端推进规则矛盾。

## What Changes
修正旧队列注释，记录源码与相关测试证据。不修改运行行为，因此 skip_specs。

## Impact
Pager turn.rs 注释和本 change 审计记录。
