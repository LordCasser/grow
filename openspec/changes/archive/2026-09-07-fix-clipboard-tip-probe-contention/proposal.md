## Why
macOS UI 元数据探测与后台图片读取共用 PASTEBOARD_LOCK，阻塞获取会把后台读取耗时带入 UI。若改为跳过忙锁，现有 poll 又会把不可用分类当成无图片缓存，漏掉后续重试。

## What Changes
元数据探测以 try_lock 获取锁，忙时返回不可用；poll 仅为具有有效分类版本的无图片结果提交去重，不缓存不可用结果。

## Impact
client-support macOS 剪贴板元数据入口、pager 图片提示去重及回归。实际粘贴读取和其串行锁保持原义。
