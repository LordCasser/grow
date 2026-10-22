# 验证记录

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

本项只消除同次恢复的重复读取/校验：根 ownership 从实际 replay 的已校验 Timeline/history 截点随接受的 load 结果传递；后代的 ownership 和有序 lifecycle/control 从一次完整 shared-read 校验生成。普通 load、resync、transport reconnect 均接入，payload 一次性消费，无跨 load cache。磁盘格式、写入/ACK、全历史就绪屏障及分页不变。

## 对照方法

实际环境为 macOS arm64、Homebrew **Rust 1.99.1**，不是仓库 pin 的1.93.1。正常 release/default CLI features、locked/offline，独占 `target/descendant-recovery-release`，jobs4、incremental关闭。先保留 before CLI/测试产物，现代 star/chain 夹具完成正确性基线后才接入生产代码；三份失败夹具记录保留且不计性能。after 最终CLI再次构建以对应最终生产源码。SHA、源码前后映射、实际编译上下文及原始日志分别见 `before/after-binaries.json`、`source-before/after.json`、`source-delta.json`、`build-context.json`。

正式测量前完成编译、scope回归与CLI热身；进程快照未发现运行中的compiler。相同测试产物/夹具、warm page cache、新进程、每组3次 AB/BA 交替。诊断默认关闭与详细 log 分开；热身/冒烟不进入中位数。RPC的全正文oracle在load计时外。共享 checkout 另有一处未由本任务修改的 actor image test 变化，`source-delta.json` 明确标记；它不进入CLI/RPC生产代码，不改写也不纳入本项。

现代根会话每轮canonical正文32KiB，admission及物理anchor；PTY另加末条可见marker。128/512/2048轮采用同样源规模，2048轮PTY timeline约74.82MB、updates=0；RPC夹具无该末条marker，4096个历史event、67,191,722正文bytes。两种harness时钟/数量不混用，不能用RPC绝对耗时减PTY绝对耗时。

## 实测结果

默认关闭诊断，单位ms，均n=3中位数；`ready_visible` 是首次marker+model label且无Loading的实际画面，位于固定500ms稳定等待之前，并非后端时间戳。`ready_ms` 此模式不可用，未当成零。原始全范围和每条观测见 `pty-default-off/metrics-summary.json`（选 `metric_type=resume-pty`，不混入 `resume-progress`）。

| 工作负载 | 末屏可见 before → after | Loading消失 before → after |
| --- | --- | --- |
| 普通 128 | 142.9 → 131.3 | 142.9 → 131.4 |
| 普通 512 | 172.6 → 194.4 | 271.8 → 253.8 |
| 普通 2048 | 550.3 → 548.2 | 800.1 → 577.1 |
| 慢 writer 512 | 279.6 → 291.4 | 324.0 → 316.2 |
| 慢 writer 2048 | 690.3 → 667.0 | 873.2 → 667.1 |

普通2048轮就绪快27.9%，范围789.2–801.4 → 576.4–610.8ms；慢writer2048轮快23.6%，范围854.5–900.2 → 662.6–697.1ms。末屏并无一致改善：普通2048基本持平，512普通/慢样本分别172.6→194.4、279.6→291.4ms，不能宣传所有场景都更快；短会话就绪范围也重叠。40ms慢writer与100ms加载绘制节奏仍保留。就绪后的输入延迟与加载期间输入忽略、Ctrl+D退出/再恢复门槛均通过。

详细log独立归因（普通2048、n=3）：

| 阶段 / 口径 | before | after |
| --- | --- | --- |
| 载入前缀内 Timeline read/decode 次数 | 每次2 | 每次1 |
| 同范围read/decode合计 | 403.734ms | 201.506ms |
| 完整 `pager.replay.finalize` 工作 | 226.287ms | 3.827ms |
| 历史 `pager.acp.apply_batch` 合计 | 193.950ms | 191.839ms |
| 首条sequence可见 | 426.5ms | 421.5ms |

前缀范围止于该次 replay completion，不是进程级所有读取次数；慢writer的原始进程汇总可包含3→2次，仍只省一处，不能写成全进程只有一次读取。after无child的 `pager.replay.descendants` 约0.004ms，capture后根文件删除仍能enrich的新测试进一步证明未偷读根。最后一条可见557.6→548.5ms，完整completion观测862.8→623.0ms；不要把嵌套phase相加，也不要将含500ms hold的stable prompt当成ready。

子树display恢复的独立release基准，4节点，root128轮+3个child各256轮，正文32KiB/轮，journal合计约32.7MB；每条边具真实canonical Spawn、匹配Seed/summary与带稳定eventId的typed presentation，断言flat display顺序、identity/cwd/worktree/seed及重复恢复不重排/重复。

| 拓扑 | before中位数[范围]ms | after中位数[范围]ms | 降幅 |
| --- | --- | --- | --- |
| star深度1 | 169.767 [163.427,170.114] | 90.835 [90.700,90.965] | 46.5% |
| chain深度3 | 169.405 [165.279,170.553] | 90.789 [90.772,97.843] | 46.4% |

该计时只含Pager descendant display restoration，**不是整树执行冷恢复**。为同一harness对照，两版都以None走独立root fallback：完整读/校验7→4次；实际已带root metadata的路径还可免去那次root读，但这部分未算进表中。每个child的完整projector仍执行一次并展开正文，不能把46%外推成全流程倍数。

理论上，这段同步展示恢复由 `Vroot + Σ(2Vi + Pi)` 变为 `Σ(Vi + Pi) + M`；V是完整节点读取/校验，P是原projector，M是新增metadata处理，fallback仍加Vroot。每次恢复依然O(全历史字节)。根无child时M很小；大型spawn/prompt树会多临时复制/序列化精简字段，保守wire上界超过1MiB时整份省略并回退，因此不保证所有树获得相同比例收益，不声称RAM/存储下降。

RPC控制是现代128/2048各3次before/after且每次完整正文/数量/顺序oracle。2048 load中位数391.5→389.7ms，first226.8→224.6、last375.1→372.3ms，基本持平；这是预期，因为本项消除UI收尾及后代重复校验，未跳过RPC的第一次完整根恢复。

## 正确性与审查

14个release scope共 **2522 passed / 0 failed**：Pager1811、Shell711。七个ignored不计通过；另明确排除与P0/P1相同的一个依赖debug_assert的release should_panic测试，未改生产断言/测试。`new-ui-checks.log`的5项与`new-storage-checks.log`的7项已含在这些scope，不重复相加。新增另1项meta解析用例，总计13项新回归。

32 Grow/7 ACP families的物理prefix对照、root/child历史顺序与80/120列渲染，以及工具/权限/状态、图片引用、raw/pretty复制、source map、布局/滚动/resize、reconnect/fork/control均经现有scope重跑。storage/history/persistence含前缀、receipt/blob/Sideband、缺失/损坏、只读不修复、strict Seed/Result/归属等既有门槛。每次child完整校验后才筛选展示，不跳过非选中消息的校验。

真实最终CLI另4项manual PTY通过：根/子/孙Ctrl+D挂起冷恢复、partial leaf release、concurrent cold claim唯一root、恢复后继续对话。核对durable seed/result/receipt、只一次命令/结果、不重放已执行工作。两种诊断模式各3对普通128/512/2048与慢512/2048、Ctrl+D退出后再恢复pair均通过。子树基准保留独立repeat恢复断言；RPC12次全历史oracle通过。基准执行/热身不额外计入unit总数。

自我对抗性审查见 `adversarial-review.md`；独立数据复核核对raw日志、中位数/范围、二进制身份与计数口径。未将展示payload升级为执行/严格跨实体链接凭据；无eventId生命周期重入重复行、展示跨实体校验策略是既有债务，已登记backlog而未混入。metadata大小上界只限制新增wire内容，不限制整个Timeline/临时map内存；不作全树原子快照、首屏加速或全负载提速承诺。仍不做就绪与历史分页分离。

## 收尾

归档前strict17/17、既有archive688/688；独立归档后主规范16/16、archive689/689，均0失败。归档命令的5/7只剩归档/清理自身行政checkbox，完成后全部勾选；proposal缺Why/What Changes标题的非阻塞提示也已补齐，原日志保留。最终SHA确认后删除专用target，释放10.162GiB并确认路径不存在；共享target、真实sessions和其他任务改动保留。详细结果见 `completion.json` 与validate/archive日志。
