## Context

`JsonlStorageAdapter::writer_leases` 是由 adapter clones 共用的 `Arc<Mutex<BTreeMap<_, File>>>`。`try_acquire_writer_lease_raw` 用 `fs2` 获取独占锁；显式释放只 remove，最终 owner 退役只 drop map。Unix flock 属于 open file description，子进程 fork 即可继承；`O_CLOEXEC` 只在 exec 时关闭，不能消除 fork 到 exec 的窗口。

## Decisions

以最小的私有 lease owner 包装已锁定 File，在 Drop 中调用现有 fs2 unlock，再由 File 关闭句柄。map 仍由原 Arc 共享，adapter clone 的 drop 不提前释放。显式 map remove 与最终 map drop 共用同一个释放边界。解锁失败记录警告并继续关闭，不能从 Drop panic。

不修改删除先获取 writer lease 的顺序，不接受任意错误替代关联 ID，不通过睡眠或重试掩盖问题。正常 Command 子进程 exec 或 `_exit`，不运行继承的 Rust owner 析构；不引入在多线程 fork 后继续运行整个 Rust runtime 的支持。

## Verification Design

回归只为一个已锁定 descriptor 在 child `pre_exec` 中清除 CLOEXEC，并用 ready 文件与 stdin 生命周期保持 child 存活。这将正常 fork 到 exec 的继承窗口稳定延长到 exec 之后，不需要在 raw fork 后执行 Rust 逻辑。child 验证 fd 存在，不创建 adapter 或取得 writer 权限。父验证：live owner 拒绝替代；仍有 clone 时拒绝替代；最终 owner drop 或显式 release 后，在 child 仍存活时允许替代；child 退出不会释放新 owner 的锁。child 始终有 kill/wait guard，ready 等待有界。

先在旧实现运行该回归得到红灯，再验证修复、原清理矩阵重复运行、JSONL 套件与 Shell 全库。按资源情况运行影响范围的检查，区分本地和远端 CI，不将原历史错误文本缺失当作已还原证据。
