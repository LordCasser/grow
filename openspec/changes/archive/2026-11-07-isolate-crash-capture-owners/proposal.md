# Isolate crash capture owners

## Why

PB-05：所有进程都用 `last-crash.bin` 并 O_TRUNC/CREATE_ALWAYS。B 启动可截断 A 的已打开 inode，后续恢复还可能删除仍被 live A 持有的文件；已归档的失败保留修复只保证单个恢复操作，无法提供进程隔离。

## What Changes

每次 capture 安装独占创建一个私有、唯一的 raw 文件，并在实际写句柄的整个寿命持有 OS 文件锁。恢复只认固定命名的本 owner capture 文件及既有单文件证据，在读取前取得原文件的非阻塞排他 claim；不能因 PID 看似死亡就删除或截断文件。恢复发布由同目录独立的恢复锁串行化，保留原 report/history 持久化边界，历史文件名加入 capture 身份以区分同秒不同 crash。正常退出留下的空文件仅在锁释放后由恢复清理。

这是 capture 文件生命周期修复，不处理跨启动 ASLR 符号化（PB-06 独立）。无用户配置格式改变；旧 raw 作为既有数据完整性入口保留，不为旧行为新建双写。

## Impact

crash-handler capture/recovery 及 CLI 启动恢复循环、对应测试与开发说明。只有显式启用 crash capture 时生效。Unix/Windows 实现共同遵守身份/claim；本机无法运行的平台单列限制。
