## Reproduction
旧实现运行 empty_skill_snapshot_survives_disk_changes 失败：冻结空正文后修改真实临时文件，读取返回 Changed after freezing，而非空字符串。

## Validation
- tools implementations::skills：97 passed，覆盖冻结后修改/删除文件、未加载文件正常读取、空 synthetic body 与缺失 body 错误等既有回归。
- agent prompt::skills：89 passed，包含空正文预加载状态保持的回归。

## Limits
本次验证 helper 与真实 agent 预加载函数，不声称执行过完整 workflow UI。已安装 grow 二进制未更新。使用关闭 incremental/debug、jobs=2 的既有测试配置；target 约 3.9 GiB，剩余磁盘约 77 GiB。
