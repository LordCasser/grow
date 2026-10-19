## Why

发布前 Core 门禁中 persistent Bash 的 set-a 回归退出 126。补充 stderr 的 Ubuntu 验证确认 base64/tr 报 Argument list too long；对源码 dump 脚本的受控复现也确认 allexport 会将序列化局部变量（含大型函数内容）自动 export，导致外部命令因环境大小限制失败。该机制本机可复现，不能依靠跳过原测试发布。

## What Changes

仅在 Bash 快照序列化期间关闭 allexport，保留原选项状态，显式清除 payload locals 的 export 属性，并在其他快照块恢复之后才恢复原 allexport；其他 POSIX/shopt 顺序保留。用户变量、函数和 allexport 语义继续跨命令保留，序列化大载荷不进入子进程环境。增加真实 prepare_command/fd/dump/replay 路径的大函数回归，不调整权限、沙箱、进程准入或 MCP。
