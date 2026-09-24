## Why

Minimal 的 `/transcript` 分帧渲染遵守每帧 8ms 预算，但最后一帧会在同一事件循环中同步写完整临时快照。受控测量中 100 MiB 快照写入中位数为 42.191 ms，三次样本最高 142.095 ms，超出渲染帧预算并延迟输入处理。

## What Changes

Minimal 完成 ANSI transcript 渲染后，将快照写入移到现有后台任务系统。只有仍匹配当前请求代次和原 root/child/session 的结果才能交给外部分页器；已替代或 owner 已失效的结果释放私有临时文件。写入失败仍反馈到原视图。

Full/TUI markdown 渲染及快照写入此次不改：当前没有归档的对应输入响应时限；测量结果仅留在验证记录。

## Capabilities

### Modified Capabilities
- client-surfaces: Minimal 外部分页器快照的写入线程、请求归属和结果生命周期。

## Impact

影响 Minimal transcript pump、Effect/TaskResult、快照写入 helper 及原 owner 反馈路径。不会改变 transcript 内容、临时文件权限、外部分页器挂起/恢复语义或 Full/TUI 执行路径。
