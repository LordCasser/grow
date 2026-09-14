## 范围与结论

本轮沿用已批准的恢复性能范围，保留 Timeline 恢复、pinned file、rewind、cursor、subagent typed validation 与 load response 屏障。没有新增运行时依赖。

Pager 的 batch append 和 dirty-height 共用已有虚拟化机制；历史新增/变动先估计，仅可见区精确测量。旧的全局布局失效不会在每次 batch 插入时收集全部历史 ID。隐藏 thinking 的跨条目间距、分组、resize 和最终 batch 收尾仍由原路径重建。Shell 的两个局部优化候选已撤回，最终代码恢复到本轮开始的版本；不对它们宣称性能收益。

## 测量方法

macOS、现有 debug profile、热 OS 文件缓存，串行运行，无并发构建。Pager 为真实 ACP handler + TextBlock，每 32 条通知 prepare_layout，前 17 个 frame 穿插输入，然后 loading 期间 Enter 入队、SessionLoaded 后发送。128/256 turns，各 turn 8 × 512 bytes。每组独立进程 3 次；同一进程先 128 后 256，128 包含初始化成本。

| Pager 阶段，中位数 ms | 128 before | 128 final | 256 before | 256 final |
|---|---:|---:|---:|---:|
| 通知处理 | 78.54 | 78.77 | 99.15 | 99.07 |
| 恢复期间布局 | 148.40 | 69.67 | 292.94 | 132.07 |
| load 收尾 | 5.85 | 5.85 | 0.98 | 1.00 |
| 最终布局 | 1.19 | 2.82 | 1.24 | 2.96 |
| 每次总工作耗时的中位数 | 234.06 | 157.99 | 394.64 | 234.06 |

总工作时间包含各次通知、布局和收尾，最终版本分别减少约 33% / 41%；最终布局多出约 1.5–1.7 ms，已计入。它不包含真实 terminal buffer 绘制、输入排队或设备回显，不能声称输入 p95 已达标。全部 user block 数、assistant 字节、草稿与队列内容均断言通过。数据位于 profiling/pager-baseline-repeats.log、pager-confirm-repeats.log 和 pager-medians.json。


Shell 先用上一批保留的 binary 对 128/512 turns、8 × 4096 bytes 做各 3 次基线，再对相同 fixture 比较。完整 load 中位数分别从 536.2/1377.4 ms 变为 588.5/1477.2 ms，未修改的 actor/forward 等阶段同样变慢，现有数据无法作可靠归因。因此撤回本轮 Shell 生产修改，数据仅保留为未采用的实验（shell-withdrawn-experiment.json），不能作为最终版本的性能结论。原始 callgraph 使用 macOS sample；首次在 fixture 完成后才附加，未捕获到有效栈，不能作为证据。第二次从进程启动采样，包含 fixture 生成与 load，不能把整个进程百分比当作 load 占比。完整树压缩保存为 profiling/shell-sample-early.txt.gz。

在 load 子树中，prepare_replay_lines 约 190 inclusive samples，含 subagent projection 83、rewind 55；stale-task scan 约 141，含 rewind 52 与两次 contains 80；snapshot 读取约 7。它支持减少冗余扫描，不证明磁盘读取是瓶颈。独立 substring 微基准中 contains / memmem find / Finder 约 1008 / 1142 / 1084 ms；未采用替换。

## 回归状态

- 新增 batch 保留测量、延后 off-screen dirty 测量、底部总高度、混合历史逐帧对比。混合参考覆盖分组、连续 hidden thinking、宽度变化、dirty text，并比较高度、间距、group flags/ranges、prompt descriptors、virtual_y 与滚动位置。
- 17 项 batch 定向测试通过。
- 最终 Pager 全量：7,201 passed / 0 failed / 11 ignored；在全部代码修改完成后重新编译并完整执行通过（pager-final-full.log）。最终同样本 3 次确认通过（pager-confirm-repeats.log）。
- 初次新增 benchmark 编译发现缺少 Duration import，补齐后通过；未调整生产代码迁就测试。
- Shell 候选阶段默认 features 全量：3,800 passed / 0 failed / 3 ignored；test-support perf target 编译通过。候选撤回后，两个 Shell 源文件 SHA-256 与上一批归档 snapshot 完全相同。
- OpenSpec 归档前全量严格校验：18 passed / 0 failed。
- 最终 CLI debug 构建通过；仅出现已有大型 debug binary 的 compact-unwind linker 警告。
- 最终 CLI 的真实 --continue PTY：1 passed，历史恰好一次显示，恢复后成功提交下一轮。
- 源码 snapshot 复核、git diff --check 通过；归档后 OpenSpec 校验结果另存日志。

## 限制与未混入的工作

未做真实长会话 terminal p95、密集工具/多子会话的性能对照、release 全库构建或大型 RSS/dhat。独立工作继续登记 backlog。stale-task repair 在 delta flush/drain 之后读文件；不能把初始 replay 快照直接当作该时刻的事实。Atlas 当前 scoped 签名陈旧，本次调用边界以当前源码和编译器验证为准。

## 磁盘与构建产物

复用原 target，Cargo 最多 2 个 build jobs，所有 Cargo 命令串行；未创建 worktree、release profile 或真实会话副本。开始可用约 61 GiB，构建期间最低观察约 51 GiB，没有触及 20 GiB 停止线。全部验证结束后，依据 build-baseline.json 与目录创建时间，只删除本任务建立的 14 个顶层 incremental 目录，删除目录的 du 统计合计约 10.53 GiB（不是磁盘可用空间的实际增加量），保留 36 个原有目录及全部依赖、测试和 CLI binary。清理前确认没有 Cargo/rustc 进程，明细见 disk-cleanup.json。

清理后系统报告可用 56.41 GiB。采样 callgraph 已压缩保存，诊断和日志合计约 2.2 MiB。
