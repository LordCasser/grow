# 外部命令资源与收尾审计

## 范围与证据状态

本记录最初核对生产入口、平台实现和 cleanup 缺陷；下述 leader-exit 缺陷已由 [`finish-command-groups-before-leader-reap`](../2026-11-04-finish-command-groups-before-leader-reap/design.md) 修复。最初的临时 `command_cleanup_probe.rs` 是调查 fixture，未运行，之后已删除；正式回归合并到 `terminal.rs` 的 `command_cleanup_finishes_term_ignoring_descendants_before_leader_reap`，`/tmp/grow-260-tools7-target-1.log` 记录 1/1 通过。资源策略仍不设置全局内存值；一次性变异副本中断脚本按 parent 运行报告 PASS，脚本与边界见下文。相关跨发布 blocker 的协调验收见 [`close-v2-6-0-release-blockers`](../../close-v2-6-0-release-blockers/design.md)。

## 生产入口与现存预算

`LocalTerminalBackend::new()` 使用默认 `LocalTerminalConfig`，其 `memory_config` 未设置；在仓内生产构造点没有找到 `with_memory_limit` 调用。后者仅由 cgroup memory 测试显式使用。Linux cgroup v2 可设置 `memory.high` 与 `memory.max`，监听 `memory.events`，并在高水位事件后由终端 actor 对新近进程组执行 SIGKILL；创建 cgroup 失败会退化为无 cgroup。macOS/Windows 的 cgroup 入口为 no-op。虽启用父级 `+memory +cpu` controller，代码未写 `cpu.max`，也没有 CPU/进程数默认配额。参见 `crates/codegen/tools/src/computer/local/{terminal.rs,cgroup.rs}`。

已有其他限制及其含义：

| 边界 | 当前值/机制 | 实际含义 |
| --- | --- | --- |
| 前台工具等待 | 默认工具 timeout 120 秒；缺省最大前台 timeout 5 分钟，session 可配置，绝对上限 10 小时 | 只限制前台等待/请求生命周期，不提供 memory/CPU ceiling。 |
| 前台自动转后台 | 默认 15 秒，可由 `GROW_FOREGROUND_BLOCK_BUDGET_MS` 配置 | 仅释放模型回合，进程继续运行。 |
| 后台最长 runtime | 10 小时 | 到期请求终止进程组；不是资源用量上限。 |
| 命令输出文件 | 默认 5 GiB，可由 `GROW_MAX_OUTPUT_FILE_BYTES` 配置 | 超限后请求终止命令；不是内存或总磁盘配额。 |
| 已保留输出 | 64 MiB | 限制终态保留/读取正文大小。 |

因此目前没有依据为“全局新预算”填入一个固定内存数值。Linux cgroup 的 `memory_high_bytes` 必须来自已存在宿主配置或后续明确策略；`CgroupMemoryConfig` 文档里的 256 MiB 只是 headroom 建议，并不是默认 `memory.high`。

## owner 生命周期与可复现缺陷

`ProcessState` 是终端侧 `Arc<ProcessGroup>` 强 owner。进程组加入 global/session `ProcessScope` 时，scope 存储 `Weak`；只有强 owner 仍活着，scope 关闭时才能升级并 kill group。Unix poll sweep 在 `child.id() == None` 后立即丢弃强引用，原意是防止后续 `killpg` 命中复用的 PGID。

调查时发现的可达失败链（现已由独立 change 修复）：

1. `graceful_kill_and_wait` 对组发 SIGTERM，但 grace wait 只等待 `Child::wait()` 的 leader。
2. shell leader 收到 TERM 后立即退出，而后代忽略 TERM；leader 在 `SIGTERM_GRACE` 内被 reap，函数 early-return，没有对仍存活的组成员发 SIGKILL。
3. `kill_and_finalize` 按 leader reap 清掉 `process_group`；scope 中的 Weak 随之失效。忽略 TERM 的后代失去可达清理 owner。

相同入口还曾有“初始 `try_wait()` 已看到 leader 退出”的 fast path：`kill_and_finalize` 返回 `AlreadyExited`，不再向组发信号。现实现先以 `waitid(WNOWAIT)` 观察 leader 终态、对 group 发最终信号并撤销 owner，再由 Tokio reaping；显式 kill 同样先发最终组信号、撤销 `ProcessScope` 注册再等待。此处只记录终端路径的局部顺序，不声称 `ProcessScope::kill_all` 已在 owner 撤销前升级的 Arc 并发交错已被同步或故障注入证明。简单延长 Arc 生命周期仍会重新引入对已复用 PGID 误杀风险。

可复现形态为：leader 安装 TERM trap 立即退出，启动安装 `SIG_IGN` 的子进程后等待；触发显式 command kill，检查子进程是否在返回后仍存活。调查期间的临时 probe 只让 shell 和 `/bin/sleep` 都继承忽略 TERM，覆盖不到 leader 早退分支；该 probe 未执行且已删除。正式 Unix 回归 `command_cleanup_finishes_term_ignoring_descendants_before_leader_reap` 覆盖 kill、timeout、normal leader exit 三条入口，并以有界后代观察验证组收尾；targeted 日志为 1/1 通过。测试只接受 descendant 已消失或为 zombie，不能证明由 PID 1 完成回收，也尚未提供其他 OS 的实机结果。

**实现与证据边界**：现有 Unix cleanup regression 已覆盖 TERM 后 leader 早退、受限后代忽略 TERM，以及 timeout/normal exit 入口；临时 integration probe 已删除，没有留下额外测试文件。目标终端测试通过记录见上方；完整 tools/package 与其他 OS 实机回归仍属于发布门禁，不能由该单项通过推导。

## 平台能力边界

| 平台 | 子进程树清理能力 | 资源预算能力 | 当前 caveat |
| --- | --- | --- | --- |
| Linux | Unix ProcessGroup `killpg`；可选 cgroup Drop 使用 `cgroup.kill` | 可选 cgroup v2 memory high/max 与 inotify 高水位监测 | 默认构造未启用 memory config；cgroup 不可用时降级；无 CPU/pid 限制。 |
| macOS | Unix ProcessGroup `killpg` | 本地 cgroup stub/no-op | 无仓内等价 memory cap；cleanup targeted 回归通过，但 `ProcessScope::kill_all` 在 owner 撤销前已升级 Arc 的并发交错未有实机故障注入证据。 |
| Windows | Job Object；TerminateJobObject，close-on-drop 清理 job members | 本地 cgroup stub/no-op | 无仓内等价 cgroup memory cap；实际 OS runner 验收仍需记录。 |

三种平台的命令 timeout/output cap 不能等同 OS 层 memory/CPU 配额。监视型 memory high 逻辑也不能表述为普适硬上限。

## 隔离协议与后续验证入口

外部变异脚本 `validate_isolated_interruption.py` 只建立临时 baseline 与 disposable trial 副本，worker 在 2 秒 ready deadline 内写入 trial 并进入受控 sleep；脚本发送 terminate，要求 worker 非零退出、baseline SHA-256 不变、trial 目录删除。已运行并由 parent 报告 PASS；没有保留独立日志。它验证中断分类和副本清理，不运行高负载，不提供内存/CPU 预算。任何真实 mutation trial 的 CPU、内存、时长和并发限额必须来自调用方/宿主配置；没有这些来源时不得启动高负载实验。普通 tool timeout、机器总内存以及 cgroup 注释中的 headroom 不能冒充预算。本 change 不新增全局内存限额。

回归入口已存在，后续需要在最终 candidate source 上重跑：

```sh
cargo test --locked -p tools --lib command_cleanup_finishes_term_ignoring_descendants_before_leader_reap -- --exact --test-threads=1 --nocapture
```

不得以已删除的临时 integration probe 作为正式入口。随后运行完整工具库回归：

```sh
cargo test --locked --lib -p tools -- --test-threads=4
```

跨平台只有相应 native runner 才能提供运行证据；无 runner 时分别记录目标平台编译和未执行的实机范围。资源预算选择应先声明宿主来源/配置，再用小型有界进程验证封顶与异常 cleanup；本审计不定义新预算数字。

变更文档核对可用：

```sh
openspec validate analyze-command-resource-budget-and-cleanup --strict --no-interactive
openspec validate close-v2-6-0-release-blockers --strict --no-interactive
openspec validate --all --strict --no-interactive
```

以上 OpenSpec 命令是 change 文档后续校验指引，本轮未执行；Cargo cleanup regression 的实际 targeted 结果见前文。
