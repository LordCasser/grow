# Verification

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

## 实现与对抗性核对

首次 Unix writer load 在 append 锁下打开 pinned directory 内的 Timeline，捕获包含 dev/inode/ctime 的原 stamp，流式解码完整行并累计原字节 BLAKE3。与普通恢复共用 evidence 校验，保留一次 fold 生成的 Timeline。经过 updates、Sideband、summary 和状态投影全部必要校验后，安装前再次以同一 directory 打开当前文件并核对原 stamp。安装不覆盖任何已确认的不同 len/hash；随后 append 仍走原 tail、stamp/hash 回退与 file/directory sync。

- 锁不跨 await；Sideband/summary 修复只修改自己的原有文件，不分配 parent Timeline seq。prefix 是函数栈上的候选与 adapter 原有缓存，不增加 owner、服务或跨 lease 证明。
- 完整原字节 hash 包含空白；未完成尾片不进入 prefix.len。首次 append 截尾后 stamp 变化，仍重新验证完整前缀，不把截断后的 metadata 贴到旧 hash 上。
- 合法的同长度改写也不能由重复 load 重新背书；候选读取后替换 inode、改写、truncate 均应在安装时拒绝。中途同 inode 变化还会由流式读取前后 stamp 核对发现。
- prompt/Sideband/lifecycle/后续状态校验失败不安装本次候选。只读 load 不种入 writer cache；其他平台缺少本项使用的 Unix 身份 stamp 时保留原验证，未声称 Windows 加速。
- 原 exact tail 在失去 ACK 或同步失败后仍重新执行 file/directory sync；缓存有效不代表可以省掉 durable barrier。schema mismatch 在 typed payload 解码之前返回。
- 没有修改历史 projector、branch/cursor、resident gate、TUI 调度或 SessionLoaded 边界。

## Release 测量

固定 Rust 1.93.1、普通 release、jobs=4、incremental=0，与 C1 相同配置。五组 profile 各测 3 个独立进程；同夹具逻辑大小保持 148178363 bytes、5 个文件。共享主机的其他负载不可控，但本项矩阵没有与本项编译并行；正确性 oracle 在 CLI 编译时运行，未计入正式性能样本。原始数据见 before/after measurements、summaries、paired-measurements.json 及对应 trace/txt。

| Profile | RPC ms | First history ms | 整进程 peak RSS bytes |
| --- | ---: | ---: | ---: |
| modern-128 | 99.680→126.597 | 37.238→41.307 | 117538816→113328128 |
| modern-512 | 232.130→226.242 | 112.178→119.044 | 268484608→261816320 |
| modern-2048 | 815.158→743.678 | 463.450→474.457 | 764002304→741556224 |
| modern-repair-512 | 415.838→408.331 | 303.151→306.994 | 277348352→267845632 |
| modern-short-512 | 132.772→123.839 | 42.127→45.641 | 140034048→131366912 |

before trace 每轮记录一次 `session.load.writer_prefix`（83.895 ms 中位数），after trace 不再出现该阶段（0 次）；现代 2048 的 `session.spawn_actor_call` 为 176.711→100.635 ms。`session.load.timeline_read_decode` 为 78.157→91.216 ms，包含新增 hash 工作。小 128 正式 RPC 有回归，必须保留。配对 AB/BA/AB 复测中，2048 的三组 before/after RPC 都更快，但 first-history 两组变慢、一组变快，首条收益不稳定；小 128 首组 before 为 423.8 ms 离群值，不用它夸大收益。

整进程 RSS 包含夹具及后置读取/验证，不是 isolated-load 峰值；数值下降不能单独解释为恢复阶段内存下降。没有独立的锁等待/占用计时，RPC 结果包含该同步验证路径的实际成本。PTY 现代 512 的三轮 visible 中位数 546.0→543.6 ms、ready 670.3→615.6 ms、echo p95 3.9 ms 不变；128-turn 样本波动明显。真实 PTY 的现代 128/512 各三轮、慢输出和加载中退出后恢复均通过，但不据此宣称首屏改善。Windows 保留原验证，本项未声称其加速；非 Unix 平台没有新增实测。

## 正确性

优化后 Shell 共 526 passed（JSONL 184、persistence 82、projection 19、storage 57、agent 132、communication history 10、transcript 42）；Pager 29 passed、1 ignored；Timeline 138 passed、3 ignored。四个优化后完整历史 oracle（modern-128、modern-2048、modern-repair-512、modern-route-512）均通过。具体独立单测及失效用例输出见 `after-unit-*.txt` 与本目录的 correctness 记录。

## 归档核对

实现、正确性及性能测量已完成。格式、git diff --check 和全量 OpenSpec strict 19 项通过。读中变化通过读前/后 stamp 代码审查和候选读出到安装之间的确定性改写/替换/截断用例核对，没有注入 read syscall 中间的并发 hook。归档前补了不支持平台的 cfg 锁守卫；macOS/Windows 分支不变，构建身份明确记录这一非 Unix/Windows 编译边界调整。C2 留存的 before 二进制和脚本在归档后清理，共享 release 依赖留给第三项；不动原有 target。
