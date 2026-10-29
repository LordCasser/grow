# 真实文件与进程恢复验证

日期：2026-10-29，macOS arm64，Rust 1.93.1。保留此前工作区全部修改。

## 发现并修复

Summary 已持久化而 replacement 未提交时，恢复写入合法 Failed。后继加载仍将“无 replacement 的 Summary”限定为未闭合事务，拒绝这个已正确终结的历史。修复前真实子进程测试在该分支报 `compaction/summary references 0 matching replacements`；[失败输出](probe-before.txt)。既有 JSONL round-trip 回归原先只执行内存 recover，现已补上将恢复终态实际写入并重新加载。

同一校验原先对整条历史按 SurfaceRange 匹配 replacement。现在只检查当前 Summary 到同 ID terminal 的事务区间，合法 Failed 可没有 replacement；之后重新压缩相同范围的结果不归给旧摘要。测试实际让新进程对同一范围提交不同摘要，再由另一进程恢复，确认两次事务的归属。产品修改限于 `storage::validate_sideband_ledgers`，没有新增持久化字段或第二份状态。

## 真实执行路径与切点

用 Cargo 构建的 Shell library 测试可执行文件作为独立子进程载体。writer 使用生产 `persistence::new → SessionPersistence → JsonlStorageAdapter`；测试 proxy 仅截住原 writer 已成功确认的 ACK，不模拟存储。该成功确认位于文件与目录 sync 之后。父进程读取物理尾事件核对 seq/payload，并确认第二个 writer 被 OS lease 拒绝，随后对其创建的子进程发送 SIGKILL 并 wait 回收。

恢复由新进程调用 `persistence::load_light(..., claim_writer=true)`、`ChatStateActor::spawn_from_validated_timeline`、`recover_interrupted_durably` 和生产 `repair_missing_control_contexts_durably`。每组再启一个恢复进程，断言 journal 字节完全不变。输出包含 writer/recovery PID、文件字节数和原 prefix hash。[真实进程结果](process-recovery-results.txt)。

| 中断位置 | 已验证结果 |
| --- | --- |
| Promoted durable、ACK 未送达 | 原 Surface 内容与 IDs 不变；一个 Failed，无 replacement/Completed。 |
| Summary durable、ACK 未送达 | 原历史保留；恢复 Failed 后可再次加载；同范围新摘要独立提交并可冷恢复。 |
| replacement durable、ACK 未送达 | 一个 replacement、一个 Completed；恢复一个缺失 AgentRole；系统头、较新 prompt/answer 的 IDs、正文与顺序保留。 |
| Control 重投影 durable、ACK 未送达 | 已写的 Control 不重复；一个 replacement 与一个 Completed。 |
| Completed durable、ACK 未送达 | 原终态直接保留，无重复 Completed 或 Control。 |
| 第一次恢复写入 Completed 后再次 SIGKILL | 后继进程保留该终态并补齐尚未发布的 Control；再恢复不追加任何 journal 字节。 |

共 6 组、7 次 SIGKILL；另有同范围重试和两个破坏性验证（均在隔离 fixture 内）。删除旧 Failed Summary 的 Sideband 证据文件仍拒绝加载；修改完整 journal 记录中的摘要文字、保持合法 JSON framing，也拒绝加载。两种拒绝均不改写父 journal。合法 Failed 的例外没有跳过来源证据、字符数、canonical prefix 或完整性检查。

## 验证命令和结果

环境：`PATH` 优先使用 Rust 与 `/usr/bin:/bin`（避免本机 DevEco 同名 diff），`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216`。

- `cargo test --locked -p shell --lib compaction_real_files_process_recovery -- --test-threads=1 --nocapture`：1 项表驱动测试通过，包含上表全部场景，2.94s。
- 同一 Cargo 产物执行 `session::storage:: --test-threads=4`：**383 passed / 0 failed / 2 ignored**，33.87s；[日志](storage-regressions.txt)。包括修改后的 JSONL round-trip、跨 ledger 语义校验、writer lease、durability 与恢复路径。既有忽略项未改变。
- 执行 `session::persistence:: --test-threads=4`：**81 passed / 0 failed**，1.74s；[日志](persistence-regressions.txt)。
- 执行 `compaction --test-threads=4`：**115 passed / 0 failed**，23.03s；[日志](compaction-regressions.txt)。含上述进程测试和前轮 ACK/取消/换模回归，不应与单项进程测试重复累计。
- 默认生产配置 `cargo check --locked -p cli --bin grow`：通过，1m58s；[日志](production-check.txt)。未关闭默认 allocator 或启用测试 feature 替代生产图。
- 新测试文件、production validator 和模块入口的 rustfmt 检查通过；`git diff --check` 通过。

## 审查与范围

独立复查确认：validator 接收的是已通过严格生命周期校验的 Timeline；Failed 携带 replacement、Completed 缺少 replacement、ID/target 不匹配与重复 Summary 均仍被拒绝。source/result 验证先于 Failed 例外，成功 replacement 的纯 CompactionMeta 与 canonical prefix 检查保留。

摘要是确定性已完成 Sideband fixture，不调用线上模型。进程内通过正式存储、ChatState 恢复和 Control 修复函数组装测试 actor；这不是公开 CLI/Leader/UI 的整套启动回归。测试证明真实文件与进程死亡边界，不代表整机断电、存储设备丢失已确认 sync，或恢复后请求 token 估计精度的全面证明。所有子进程都有有界等待和 kill/wait 清理，子进程临时目录随隔离根目录删除。

## 收尾

- OpenSpec 已归档；归档后全量严格校验 16 passed / 0 failed，archive 校验 720 passed / 0 failed。
- 最终 `git diff --check` 通过。
- `cargo clean` 删除 12,803 个构建文件，回收 3.1 GiB。进程测试只终止自己创建的子进程，未终止用户会话或其他项目进程。
- 未提交、推送或发布二进制；修改留在当前工作区。
