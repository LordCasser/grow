## Scope
locked/offline、无增量、debug=0、2 jobs 构建。三个只读启动检查不读取真实会话或写剪贴板，不替换用户安装目录。

## Evidence limits
模块回归覆盖具体复制行为；CLI smoke 只证明链接和入口可运行。SHA-256 区分同一 Git 哈希下的工作树构建。构建前 target 11 GiB、磁盘可用 66 GiB，尚无立即 clean 的空间压力。
