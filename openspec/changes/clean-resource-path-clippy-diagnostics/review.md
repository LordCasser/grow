# 对抗性复核

- 34 个同步 canonicalize 调用改用已有 dunce，6 个相关 crate 均已有依赖。保留输入、错误上下文、fallback 与所有断言。两个 Tools 异步调用复用 try_canonicalize，不使用吞错误/超时回退助手；Shell 单点异步入口在把路径传给 read_dir 前 simplified，与现有 Workspace 同类帮助函数相同。没有用字符串替换做 symlink containment。
- Agent rebuild 的 RwLock 与 Workflow tracker 的临时锁原来处于 await 表达式中，改为先取得独立快照。通知持久化的 guard 仍覆盖 owner 快照、预算和入队，作用域在等待 ACK 前结束；不能把 owner clone 单独提前解锁，否则可能破坏归属与派发的线性化边界。
- 四处测试显式 drop 改词法块，块内断言和相对 await 顺序不变；没有提前释放用于环境串行化的全局测试锁。closure 的显式 drop 被 NLL 的最后使用点替代，没有删除三个控制领域的安装或改变 sequence 上界计算。
- 两处日志 OpenOptions 显式 truncate(false)，保留已有字节和 inode，既有锁内 seek/trim 流程不变。不能用 truncate(true) 消除诊断。
- 十个进程调用保留局部 expect，而不是新增适配层：八个是显式 wait 或 kill/reap 的跨进程测试；OCR 有输入/输出/时间上限、try_join wait 和 kill_on_drop；Trajectory 有独立 readiness/runtime supervisor、wait 和 kill_on_drop。将这些调用强行改为 session Scope 会改变独立 helper/测试的所有者与生命周期，超过本项。原有 OCR 只保证直接子进程取消，不在本项宣称扩展到任意后代树。
- 可选 Tokio process 方法的 allow-invalid 只处理 feature 关闭时路径不可达；不允许实际禁用调用。隔离探针分别检查缺省 feature 的零诊断，以及启用 process 后原始 spawn 被 -D warnings 拒绝。
- expect 是有理由的局部规则例外，未改全局 lint level；例外不能被描述为重写了进程生命周期。第一次严格检查发现三个 expect 被错误贴到测试内嵌 Drop impl，触发未满足期望，同时原 spawn 仍报错；现已把它们移至准确的 spawn let，保留失败证据。

补正上一项验证说明：独立 check 命令是 -p pager，而不是 cli 组合根。全 workspace all-targets 曾包含 cli，但不能等价于独立默认 feature check；本项另跑 -p cli check、Clippy -D warnings 与 build/link。
