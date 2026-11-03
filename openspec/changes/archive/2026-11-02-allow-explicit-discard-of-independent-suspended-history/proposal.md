## Why

Inactive 独立挂起 session 的永久删除被 unfinished runtime handoff 护栏拒绝，用户只能先尝试恢复再删除；恢复受阻时成为死路。确认永久删除本身就是放弃该独立 session 的本地续跑责任，与自动 TTL 的保留义务不同。

## What Changes

独立 session 在既有 writer ownership、canonical 引用和 quarantine 身份检查通过后，允许显式永久删除未消费的挂起或恢复中历史；quarantine commit 是其本地 continuation 被放弃的边界。不新增执行释放事实，不启动恢复，不删除其他 session。TTL、普通恢复、导入和 rewind 继续保留各自挂起约束。

## Capabilities

Modified: `session-timeline`。

## Impact

仅调整显式删除的 runtime 资格。真实 actor 的停止/drain 仍由现有 ACP lifecycle 负责，后续删除结果收敛增量继续完善失败路径。本轮不操作用户历史。
