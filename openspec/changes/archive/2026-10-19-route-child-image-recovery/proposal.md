# 路由 child 图片恢复事件

## Why
ImageRecoveryOperation 已接入 root session notification 投影，但 child 通知分流白名单及 child handler 都没有处理该事件，child view 因此看不到后端实际执行的图片恢复生命周期。

## What Changes
将 child 的图片恢复 lifecycle 按 child session 路由到 child scrollback，并复用 root 使用的同一行投影逻辑。

本 change 的 `client-surfaces` delta 累积 `merge-image-recovery-reconnect-rows` 对同一完整 requirement 的 cursor reconnect 契约与场景。归档顺序要求先归档 `merge-image-recovery-reconnect-rows`，再归档本 change，避免后归档的完整 requirement 覆盖已归档的重连场景。

## Non-goals
不改变 lifecycle 通知协议或将图片恢复行提升到 parent view。
