## Why

Pager 只根据剩余的非 viewer 页面决定是否请求挂起；页面状态不是 runtime 所有权。共享 leader 的退出请求没有客户端 scope，且所有客户端复用一个 pending exit，可能挂起另一客户端的 root。与 close 并发时 sessions map 已移除但 writer 仍在 drain，退出不能漏掉它。

## What Changes

- 已初始化的 Pager 退出统一向 runtime 请求确认，包括 Welcome/仅 viewer 页面；启动身份未确认时显示现有 Unconfirmed/retry，保留 startup owner。
- leader 从现有 session_driver 派生并覆盖可信 scope，backend 按连接区分 exit owner，并只处理其 roots。直接进程模式保持全 runtime scope。
- 退出在既有 lifecycle transaction 中覆盖 live 与 draining writer；沿用现有 deadline 与 tree checkpoint，失败不得宣称可退出。
- 全局 Quit 终止当前命令前缀，避免中断后误确认关闭。

## Capabilities

Modified: `client-surfaces`。

## Impact

Pager 退出分派、leader 转发与 backend 挂起 owner。复用既有树挂起和 session_driver，不新增页面所有权表，不改变 checkpoint/恢复身份。
