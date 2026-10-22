# R5/R6 后的首条历史与 release 恢复

本次只分析现有 JSONL、Timeline 和 ACP 路径，不引入 SQLite。恢复仍完整校验历史，按实际吞吐量交付全部可见历史；没有跳过正文校验、降低同步等级或使用预折叠快照。

## 6.69 秒实际包含什么

取 R6 的现代 2048 轮、总耗时中位数所在的同一次运行：load RPC 9.861964 秒，CountingClient 首条历史 6.691948 秒，最后历史 8.375453 秒。这是 Rust 1.99.1 未优化 debug，不是 TUI 首屏，也不是 release。

| 首条历史之前的串行阶段 | 毫秒 |
| --- | ---: |
| `load_light` | 4251.930 |
| replay 读取完整物理快照 | 26.147 |
| replay 重整、metadata/cursor/通信历史准备 | 718.221 |
| 同步解码并入队全部历史 | 1575.800 |
| 以上合计 | 6572.098 |
| 到首条客户端历史的剩余差值 | 119.850 |

四个阶段互不重叠。`load_light` 内的 canonical 构造 2727.964 ms、Timeline read/decode 931.047 ms、physical updates decode 552.546 ms 已包含在 4251.930 ms 中，不能再相加；主 Timeline fold 只有 15.775 ms。历史只有 duration 聚合，119.850 ms 不能精确命名为某个锁、sleep 或网关函数。见 [同轮拆分](debug-first-history-breakdown.json)。

`replay_session_updates` 先读取全部 updates 快照、做完整 reconciliation，再进入没有 await 的 `forward_raw_replay_line` 循环。`forward_with_completion` 只是向 gateway channel 入队；gateway receiver 与 load 使用 LocalSet，循环结束后 `rx.await` 才给它执行机会。因此第一条已入队，不代表客户端已收到；这 1.58 秒同步循环也进入首条历史延迟。

R5 的独立 writer 前缀验证在 seeded route 样本中发生于初次 replay 后的 actor 建立，影响 ready，却不能用于解释这个样本的 6.69 秒。缺模型路由基线时，补写会把这项工作提前；这是工作移动，不能算作消除。

## 原基线中的重复工作位置

1. Timeline 通用 version reader 先收集 `RawValue`，为 version 再建整棵 `Value`，然后从原始 JSON typed decode；`TimelineEvent` 的 wire 又把 event 存成 `Value` 再转换成 kind。完整语义校验不能省，但 schema dispatch 不需要复制一整份正文树。
2. 冷 writer load 已 typed decode physical updates、核对 canonical，之后 replay 又读取 raw snapshot、做 metadata 多次扫描、再解码 projection。复用范围应限定同次冷 load 的 pinned 文件、完整截点和已确认 repair，不能跨 load 或直接套到 resident delta。
3. `forward_raw_replay_line` 将 projection 解码后展开成 typed notification，却又 `json!` → String → 递归 RawLinePeek → typed SessionNotification。这里有明确的可消除往返；最终 ACP wire 的序列化仍然需要。
4. 独立 writer 首次 append 仍读/hash/解码/校验完整前缀。R5 已消除累计生命周期复制；如果以后复用验证，应传递同一打开文件与物理前缀的验证结果，不能只用相同 seq 或内存 Timeline 充当磁盘完整性证据。

首条历史的低风险调度切口是在完整校验与排序之后按字节/记录数有界交付。每批释放执行或等待实际 completion，再交付下一批，末尾仍等全部 completion 才返回 load。单纯 `yield_now` 只改变调度，不保证内存有界；也不能在 resident delta 的同步入队/gate 边界中随意加 await。

## 测量边界

release 使用仓库固定 Rust 1.93.1、普通 `--release`，不是 release-dist；关闭 incremental，不更改优化级别、LTO、代码生成单元或持久化设置。正式计时直接执行已编译产物，不与本任务构建/正确性运行重叠。主机仍有用户应用和 VM，并非独占。夹具刚生成，文件页缓存温热；结果不是冷盘 I/O 基准。

现代夹具沿用 R5/R6：合法 request completed/admission、空正文 candidate anchor、生产 projector 生成的 projection/digest，当前 mock route 基线；每 turn 一个 user 和一个 32 KiB assistant（短正文为 4 KiB）。初始化、生成和完整历史 oracle 位于 RPC 窗口外。128/512/2048、512 短正文与缺 32 条 projection 各三轮，逐轮保存 stdout 和完整 instrumentation 时间线。真实 PTY 的 catalog 负载更大，不能把 PTY 与 Shell 差值当作纯绘制成本。

## Release 实测与两项独立优化

不是把 debug 的 6.69 s 直接按比例折算。本轮普通 release 的原始 modern-2048 基线为 RPC 943.225 ms、首条历史 550.976 ms；C1 移除解码中转及 typed/JSON 往返后为 765.529 / 424.361 ms。随后重新测得 C2 基线 RPC 815.158 ms，prefix 复用后为 743.678 ms。两项来自各自独立前后矩阵，不叠加不同批次的百分比，也不把 debug/release 的全部差异归因于 C1/C2。

原始结果、负面样本与逐场景核对分别保存在 [C1 验证](../2026-10-21-streamline-resume-codec-and-projections/verification.md) 和 [C2 验证](../2026-10-21-reuse-validated-timeline-prefix/verification.md)。C1 的 repair 样本变慢，正常 512-turn PTY 也较慢；C2 的 128-turn RPC 变慢，首条历史没有稳定改善。不能概括为所有场景提速，也不能以整进程 RSS 宣称 isolated-load 内存下降。

## C2 完成后、C3 实施前的首条历史拆解

取 C2 优化后 **RPC 中位数所在的同一次 modern-2048 运行**，RPC 743.678 ms、首条历史 474.457 ms。时间线见 [同轮原始区间](../2026-10-21-reuse-validated-timeline-prefix/after-timelines.json)，不是分别挑选各阶段最快样本。

| 首条历史前已串行完成的阶段 | ms |
| --- | ---: |
| `load_light` | 320.355 |
| replay raw snapshot | 34.529 |
| replay read/reconcile | 87.123 |
| 同步 forward updates | 23.479 |
| 以上互不重叠阶段合计 | 465.486 |
| 启动、阶段间隙及首条客户端交付余量 | 8.971 |

`load_light` 内 Timeline read/decode/hash 91.216 ms、physical updates decode 73.367 ms、canonical 构造 139.007 ms。它们已经包含在 320.355 ms，不能再叠加。writer prefix 已无第二次读取；actor call 100.635 ms 发生在初次 replay 之后，不能解释首条历史。C2 的主要收益是 ready，首次必要读取还增加了原字节 hash 成本。

当时的剩余优先级是去掉 canonical digest 的重复序列化、projection 正文的重复存储/解码/比较，并保留原字节完整性检测。随后这些方向进入独立的 C3 change，不能再当作当前未实施事项。完整 fold 在该样本约 3 ms，缓存 fold 仍不是主方向。就绪与历史分页分离没有实施，也不列入本轮后续工作。

## 第三项的实施前发现

第三项（C3）已补五组各三轮恢复基线及三个进程的正常 response 提交基线，并按确认范围实施统一物理日志。最终运行时验证与配对结果已完成，见 [C3 验证](../2026-10-22-unify-timeline-history-storage/verification.md)。实施前最新 2048-turn RPC 为 754–1085 ms，显示共享主机波动；正式比较需使用保留的 before executable 交替复测，不能选择较高基线夸大收益。

32 KiB response 的进程内提交 median 为 41.215、31.235、41.405 ms；窗口是 anchor + admission + projection，不含完整 turn。每次两个 updates 加一个 Timeline durable append，共六个 ledger 同步探针点，并非全进程 syscall 数。删除 projection 提交的潜力明确，但无需据此假定必须把所有展示通道搬入 Timeline。

全量来源盘点发现 updates 还拥有唯一展示内容，完全合并需要补充 typed 展示事实并迁移 producer 的 owner 通道。C3 范围复审比较了完全合并与精简展示账本。用户已要求继续原第三项的完整合并方案；已实施方案使用闭合 typed 展示事实与 Domain 共用物理日志，不引入 SQLite、双写或提前 ready。C3 最终验证与正式数据见上述验证记录，本分析的旧基线不作为优化后成绩。


最终 C3 三对 release A/B：2048 轮 load RPC 747.1→416.7 ms、首条 ACP 历史 438.3→232.3 ms、目录148,178,363→74,849,404 bytes。统一 journal 解码/校验约201.7 ms 是当前首条历史前的主要剩余成本；fold约3ms，继续缓存 fold 对整体收益有限。真实PTY ready略有改善，末屏/慢writer没有一致加速，常规峰值内存也没有确证下降。完整 min/max、durable提交和反例见 [C3 最终配对报告](../2026-10-22-unify-timeline-history-storage/paired-analysis.md)。保留完整校验和全部历史后ready；不引入SQLite或就绪/历史分页分离。
