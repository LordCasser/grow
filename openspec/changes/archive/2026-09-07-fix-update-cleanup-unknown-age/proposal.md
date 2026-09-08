## Why
版本文件清理将未来时间戳或元数据读取失败视为非新文件，可能在系统时钟回拨后删除并发更新刚安装的二进制。临时文件路径却在相同情况下保留，两个年龄判断不一致。
## What Changes
正式版本文件只在成功读取 mtime 并确认年龄超过 STALE_TMP_AGE 后删除。保留已有版本筛选和新文件保护。
## Capabilities
### New Capabilities
无。
### Modified Capabilities
- `client-surfaces`: 下载目录回收需有明确过期证据。
## Impact
update 的版本文件清理判断、真实文件回归和说明；不改变符号链接切换、保留版本数量或引入锁。
