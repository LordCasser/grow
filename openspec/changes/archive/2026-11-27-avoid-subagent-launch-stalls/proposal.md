## Why

实时 SubagentSpawned 在 Pager 的事件线程读取并验证完整父 Timeline，以取得三个已有的展示字段。现场一个约 734 MiB 的父会话中，29 次全新委派从启动展示记录到首次输入或失败耗时 19–30 秒，并触发过启动通知的回执超时。恢复委派还会在 Shell 调度线程执行来源存储校验。

## What Changes

- 从已提交的 Spawn 投影任务提示、实际 cwd 和 worktree。实时启动不再打开父历史，已知没有继承展示的启动也跳过子历史读取。
- 将 `resume_from` 的同步来源存储校验移出 Shell 调度线程，保留校验与启动顺序。

## Impact

影响 Grow 扩展通知、Pager 启动展示和 Shell 恢复来源读取。沿用现有 Timeline 格式及发布回执。
