# 调查与实施边界

资源预算必须归到宿主拥有的执行边界，异常收尾必须由持有 OS child / group 的终端 actor 完成。模型命令文本不是可靠的负载身份，不能按字符串黑名单或更长 timeout 作为安全控制。

本轮分开验证已具备能力、静态可达缺陷和低成本进程验证；不得将监控式软保护描述为内核硬上限。分析、验证和后续回归方案集中记录在本 change。

## 外部变异实验隔离

可能改写数据或消耗大量资源的实验只在一次性临时副本中执行；baseline/source checkout 和原始输入保持只读并留存。每次试验的资源和时间预算必须来自调用方或宿主明确设置的限制；缺少预算来源时不启动高负载实验，也不从普通命令 timeout、机器总内存或 cgroup headroom 注释推导全局预算。预算到期只表示试验未按时完成，不能算作 mutation 通过、成功清理或稳定性结论。收到中断、超时或 runner 退出时，终止并回收该次 trial 的进程树，丢弃临时副本和中间产物；baseline 保留以供重新建立干净 trial。此流程不新增产品级全局预算或运行时策略。

## 预算选择边界

默认 `LocalTerminalBackend::new()` 使用无 `memory_config` 的默认配置。生产构造点没有调用 `with_memory_limit`；当前 cgroup memory tests 的显式配置不能视为生产默认值。Linux cgroup v2 可配置 `memory.high` 与 `memory.max = high + headroom`，并通过 `memory.events` 触发整组 SIGKILL；配置创建不可用时会退化为 no-op。macOS 与 Windows 没有此 cgroup 实现。headroom 注释中的 256 MiB 是建议值，不是可直接采用的默认内存预算。

现有执行界限包括前台等待与超时、自动转后台、后台最长运行时间、输出文件上限和保留输出上限。自动转后台不会限制进程资源。代码没有 CPU 配额、进程数配额或默认内存硬上限。新增全局预算须等待宿主/用户策略来源，不得从工具超时、当前机器内存或 headroom 注释推算一个固定值。

## 进程组 owner 与 cleanup 缺口

终端 `ProcessState` 持有 `Arc<ProcessGroup>`；global/session `ProcessScope` 只保存 `Weak`，owner 持有强引用时 scope 才能在关闭时升级并杀组。Unix 在 leader `child.id()` 变为 `None` 后清除强引用，以防对复用的 PGID 执行 `killpg`。

显式 kill 的 `graceful_kill_and_wait` 先给整个组 SIGTERM，但宽限期只等待 leader。若 leader 在 1 秒内退出，函数提前返回，不发 SIGKILL；`kill_and_finalize` 随后按 leader 已 reap 清除组 owner。忽略 TERM 的后代仍可能存在，而 ProcessScope 的 Weak 已失效。另有 `kill_and_finalize` 初始 `try_wait()` 已发现 leader 退出时的 fast path：直接返回 `AlreadyExited`，没有终止组。当前 `send_sigkill_to_group` 在 Unix 也会因 leader 已 reap 而跳过 killpg。

修复需把“leader 已 reap”与“进程树终止责任已完成”分开，同时保留现有 PGID 重用保护。安全边界应由持有组句柄的一方完成最终组信号，再释放可供 ProcessScope 使用的强引用；不能只延长 Arc 生命周期，也不能依靠 leader 的 exit status 推断后代已退出。具体信号时序由独立行为 change 和源码修复决定，本 change 不改实现。

调查期间存在过的临时 `crates/codegen/tools/tests/command_cleanup_probe.rs` 让 shell 与子进程都忽略 TERM，只覆盖超时升级路径，没有隔离 leader 先退出的反例；该 probe 未运行，现已删除。正式回归已合并到 `terminal.rs::command_cleanup_finishes_term_ignoring_descendants_before_leader_reap`，`/tmp/grow-260-tools7-target-1.log` 记录 1/1 通过。变异工作流的资源/时长预算由调用方或宿主显式提供；隔离脚本的 disposable-copy interruption 检查已 PASS（parent 报告，无独立日志），不构成全局内存预算，也不代表资源封顶。没有预算来源时不运行高负载实验；本 change 不新增全局内存限额。
