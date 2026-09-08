## Why
macOS AppleScript 读图回退共用 temp_dir 下固定 grow-clipboard-probe.png/tiff/jpg。并发调用或不同 Grow 进程会覆盖/删除彼此产物，文件还依赖分支式手工清理。

## What Changes
每次逻辑回退分配私有 TempDir，由调用者持有到脚本执行和读取结束。三个路径只在该目录内使用，不再共享全局文件名；目录 Drop 覆盖失败和早返回的残留清理。

## Impact
client-support macOS get_image/get_attachments 的 AppleScript 回退。保留原生读图及文件 URL 优先规则，不删除可用的兼容回退开关。
