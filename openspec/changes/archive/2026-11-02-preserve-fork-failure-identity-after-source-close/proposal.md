## Why

移除 source AgentView 会清掉其余页的 forked_from 本地引用；worktree fork 随后的初始失败若仅按此引用识别，将误入普通创建失败并丢 pending directive。

## What Changes

以已有 pending_fork_banner 保留的请求身份判定初始 fork 失败，来源页退休不解除失败回调的内容归属。

## Capabilities

Modified: client-surfaces。

## Impact

Pager 创建失败路由和 late failure 回归；没有新状态、存储或 runtime 操作。

同一未完成 fork 若已复制、随后初始 replay/load 失败，仍保留 directive/草稿；清除失败 binding 后复用同一失败落点，不删除可能已提交的副本。
