# Design

锁先取值快照再 await。显式 drop 不能替代词法作用域对 future 状态的约束；通知的 sampling_preview 锁必须覆盖归属快照、预算获取和持久化入队，再在 ACK await 前离开作用域。测试的互斥区保留所有原断言及其执行位置。

同步路径使用 dunce::canonicalize；Tools 使用现有 try_canonicalize，保留 io::Error。Shell 单点异步路径先 canonicalize 再 dunce::simplified，与现有 Workspace 帮助函数保持同样的规范化策略；该点用精确 expect 标记允许原始调用仅用于安全包装，不新增公共助手。现有 symlink containment/失败拒绝保持。

日志 open 使用 truncate(false)，保持已有内容和 inode。调用的进程必须有现存 owner，记录监督、取消、kill/wait 证据；本项不新增进程框架或更改生命周期。测试直接派生子进程时保留跨进程并发与 crash barrier。

固定 Rust 1.93.1、独立 HOME/GROW_HOME；全 workspace all-targets Clippy -D warnings、fmt/check/默认 CLI build，受影响测试和最终 release 恢复/TUI 检查。核对可选 Tokio 规则在 process feature 开启时仍拒绝原始 spawn。归档后清理本轮专用 target，不动共享缓存。
