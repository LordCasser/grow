## Approach
沿用 locked/offline、关闭增量与 debug info、两个 job 的构建配置。编译期间不改 Rust。版本和帮助在会话初始化前退出，不启动真实会话。

## Limits
不以链接烟测代替交互 UI、ACP 写盘或真实文件系统阻塞测试。
