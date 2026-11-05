## Why

WS-03实际公共API已确认：fast-worktree删除checkout后，无条件递归删除.git文件指向的目录，separate Git元数据及伪造的无关目录均被误删。显式删除checkout不授予其指针目标删除权。

## What Changes

把registration解析/验证放回fast-worktree的Git所有者，只有结构、common-dir和backlink属于同checkout的linked registration可随checkout清理。不验证的gitfile目标保持，合法linked注册正常清除；失败不得静默报告完整成功。普通工作树入口、workspace force和失败清理仍使用同公共删除API，不另造外围guard。

## Capabilities

### Modified Capabilities

- session-timeline：工作区资源删除范围与linked registration身份。

## Impact

fast-worktree删除owner及相关Git helper/tests；不改Linux overlay/btrfs既有身份协议，不操作用户真实工作树，不提交或清理当前dirty。tier3公共API/对抗指针和独立review；属于整体backlog数据完整性里程碑。
