# Session resume：算法成本与连续加载

用户的取舍已经明确：加载足够快，可以接受少量 TUI 卡顿；加载需要几秒甚至更久，就应该看到历史从开头快速向后推进，不能滚动到一半长时间停住。这里同时记录速度优化和可见进度方案，尚未实施。

## 恢复链路与边界

当前恢复大致经过：配置/信任准备 → 读取并校验 Timeline → 修复持久投影、准备历史 → 转发历史与后续增量 → 恢复运行时及子任务 → SessionLoaded 放行。入口见 `crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs:638`，历史转发见 `crates/codegen/shell/src/agent/mvp_agent/mod.rs:978,1134`；存储加载见 `crates/codegen/shell/src/session/storage/jsonl/mod.rs:592`。

这些阶段不能随意合并。初始 physical updates 用于恢复 event identity 上界，即使 noReplay、cursor 或 rewind 隐藏了对应内容，也必须计入；后续 flush 和 stale-task 处理可能产生新的 durable facts，所以后面的增量读取不一定是前面读取的重复。Timeline 完整校验通过前也不能把任意历史当作有效会话发布，后面的 rewind 和 projection 决策会影响前面的显示。

相关权威是 `openspec/specs/session-timeline/spec.md` 的 bulk restore、response projection、exact replay barrier、physical event identity 条款，以及 `openspec/specs/client-surfaces/spec.md` 的 replay repaint 与 loading draft 条款。历史测量只解释当时实现，不替代这些契约。

## R1：Timeline admission 校验仍会反复扫描旧事件和 Surface

`Timeline::from_events`（`crates/codegen/chat-state/src/timeline.rs:2021`）已经使用私有 bulk fold，避免旧版逐事件事务克隆。但 `validate_messages`（`:4644`）对每个带 response admission 的消息仍然执行：

1. 从 events 尾部查找同 request 最近的 Completed（`:4660`）。
2. 扫描全部旧 events，确认 admission identity 未使用（`:4673`）。
3. 克隆完整 Surface，追加新 items，再执行 quarantine，只读取返回的数量（`:4683`）。

设 E 为事件数、R 为响应接纳数、S_i 为第 i 次接纳前的 Surface 长度，额外成本为 O(R×E + ΣS_i)，另有消息载荷处理成本。普通追加历史可以接近平方增长。Compaction 能缩短 Surface，但不会消除 physical events 的增长。

最小实现顺序：先在 fold 所有者内维护 `request_id -> latest completed attempt` 和已接纳 identity 集合，把前两项变成平均 O(1) 查找；再为 quarantine 提供只读计数路径，消除候选 Surface 克隆和不需要的 evidence 序列化。后者先降低分配/常数，仍是 O(S_i) 扫描。只有测量确认后，再维护 active Surface 的 tool identity 摘要，使普通 Append 只处理新 items；Rewind、Replace、Compaction 时重建摘要即可，不引入通用回滚框架。

`LifecycleFold` 已有部分身份集合，但生命周期中见过的 tool 与当前 Surface 中仍有效的 tool 不是一回事，不能直接复用错语义。新索引只能随已接受事件更新；实时 durable write 失败不能污染状态。冷 bulk fold 失败可以整体丢弃私有状态，但不能因此改变 live 路径原子性。

对抗性验证：同 request 多 attempt、重复 admission、无 Completed、跨 append 的重复 tool id、空 tool id/name、rewind/compaction 后的身份、失败写入重试。对当前 full validator 做结果/错误位置差分，比较 128/512/2048 轮增长曲线。证据置信度高；当前总耗时占比未测。

## R2：投影规划已经知道“缺哪些”，调用方却重新查整份历史

`load_light_data` 把 physical updates 整体 clone 给 `reconcile_response_projections`，随后对每个返回 projection 再 `existing.iter().any(exact_match)`（`crates/codegen/shell/src/session/storage/jsonl/mod.rs:637-665`）。P 个 projection、U 条 update 带来 O(P×U) 查找，另外还有 clone 和正文比较。

`plan_response_projections`（`crates/codegen/shell/src/session/response_projection.rs:153`）已经构造 identity/anchor/exact indices 并识别缺失项。最小方案是复用这份计划，给加载方返回需要持久追加的 projection，避免生成完整 reconciled history 再搜索。现有 BTreeMap 规划仍有 O((U+P) log P) 量级工作和载荷核对，不能把它写成整个恢复都变成 O(N)。

同一 Timeline 上 `admitted_responses`（`timeline.rs:2092`）和 `response_projection_tail_safe`（`:2117`）还会分别计算 branch provenance；missing-anchor 分支可能再次调用后者。只在同一不可变 Timeline revision 内共享分析结果，可以去掉这些重复 fold。Timeline 增加 durable facts 后必须重新计算，不能跨恢复阶段复用旧结果。

保留 `exact_match` 的完整比较：version、session、identity、digest、disposition 和 updates 都有意义；相同 identity 但不同正文必须失败。不能仅用 digest/identity 跳过冲突，不能丢弃被 cursor/rewind 隐藏但已占用的 event identity。测试复用 `response_projection.rs` 中 exact/conflict/anchor/rewrite 用例，并增加多 projection 全部存在、部分缺失、多个缺失 anchor 的规模样例。证据置信度高。

## R3：减少 JSON 中间表示和重复序列化

`read_versioned_jsonl_from_directory`（`crates/codegen/shell/src/session/storage/jsonl/mod.rs:2521`）先保存 RawValue，再为 version 建立完整 `serde_json::Value`，最后解码目标类型。可先消除完整 Value 树，按记录读取轻量 version header/typed data，减少中间保存。若 header 与 typed 仍分别解析，就仍是两次解析，不能称为一次扫描。

`forward_replay_line` 的 projection 路径（`crates/codegen/shell/src/agent/mvp_agent/mod.rs:978` 起）把已经解析的 projection 转成 typed notification，再转 JSON/字符串并递归进入解析路径。复用 typed notification 的转发入口可以去掉这段往返。R3 主要降低 O(B) 字节处理的常数和峰值内存，不改变复杂度阶数。

保留 version 错误分类、行位置、torn tail、最大行长度、session identity、rewind 与 cursor 过滤、相同 gateway 顺序。Prompt blob 已去重验证引用；必须读/校验的内容存在 Ω(B) 下界，不把完整性检查算成冗余。验证应对等比较正常/损坏 JSONL、unicode、超长单行、投影与普通 ACP 混排输出。证据置信度高，收益待 profile。

## R4：布局减少对历史长度的依赖

与 [UI 审计 U1](ui-audit.md) 是同一候选，不重复计数。当前 clean frame 仍求和全部可见历史高度；exact measurement settle 会重建所有 offsets；dirty patch 还扫描全部 prompt descriptors。

先利用现有 `virtual_y` 的范围端点计算总高度，避免 clean frame 的 O(N) 求和；高度变化与结构变化分开，只有分组、折叠、宽度等真正变化才走全量重建。尾部变化只修正对应区间和 prompt descriptors。现有前缀数组足以解决的部分不先换 Fenwick tree；只有随机历史修改的 profile 支持，再评估树结构。

## 加载过程如何可见

当前已经有 Loading 状态、spinner、ACP 批量消费、输入优先和至少 100 ms 的 replay 自动重绘间隔（`crates/codegen/pager/src/app/root/event_loop.rs:2519`）。Markdown replay 也已有 deferred 插入和懒测量。不能把“加 spinner”“每条消息立即刷屏”当成新方案；后者可能让总加载更慢。

建议另开 UI 行为 change，按下面两段处理：

- 完整校验尚未结束：显示真实阶段与可获得的已处理字节/事件数。此时可以更新进度，但不伪造已经恢复完成的历史，也不承诺精确百分比。
- validated replay 开始后：按时间预算消费 FIFO 历史，已有 backlog 时定期提交一帧，恢复从初始历史快速向后滚动。不要人为加动画延迟，也不要每批等待 UI ACK 才生产下一批。保持输入处理、宽度变化与取消可响应，接近完成时及时提交末帧。

显示“仍在工作”和实际内容推进是两个指标。应新增 `first_history_visible`、有 backlog 时相邻内容推进的最大间隔、`last_history_visible -> ready`，继续保留总耗时和按键 p95/max。可先以 300 ms 最大内容停顿作为实验目标，需按终端能力验证后再定契约。不能用 spinner 帧或按键回显代替内容推进证据。

先测同步前置/收尾阶段：TUI load effect 的 `load_mcp_servers`（`crates/codegen/pager/src/app/root/effects/mod.rs:883`）、session load 中 envrc 和 actor/tree restore，以及 `handle_session_loaded` 的 allocator purge（`crates/codegen/pager/src/app/root/dispatch/session/load.rs:690`）。搬离 UI 线程只能改善响应性，不算算法加速，也不能在未测量前认定它们是主因。

## 已有测量能说明什么

以下全是历史归档结果，本次没有重新运行。路径均位于 `openspec/changes/archive/`：

| 记录 | 结果与限制 |
| --- | --- |
| `2026-09-10-accelerate-long-timeline-replay/verification.md` | 66,069 事件，194 MiB Timeline / 98 MiB updates；debug 下旧事务 fold 88.316 s → bulk 1.543 s；JSON 解码仍为 22.394 s。57.24× 仅指 fold，不是整体 resume。 |
| `2026-09-12-optimize-session-history-replay/verification.md` | actor 局部提升约 19%，128-turn 整体约 2%；增大 drain batch 的实验没有改善，已撤回。 |
| `2026-09-12-accelerate-session-replay/verification.md` | lazy layout 的历史优化已经存在；另一些 shell 扫描实验结果不支持保留，不能重复宣传为待做收益。 |
| `2026-09-24-throttle-resume-replay-paint/verification.md` | 128/512-turn 历史可见 3046/9354 ms → 2352/6108 ms；render 次数 112/253 → 47/71；按键 p95 7.7/7.8 ms。各阶段计时有重叠，不能相加。 |
| `2026-09-24-measure-resume-input-during-replay/verification.md` | 512-turn、40 ms writer delay，20 次按键都发生在末条历史出现前；p95 77.6 ms，最终历史 7.40 s。证明 loading 时可输入，未证明每个时段历史都连续推进。 |
| `2026-09-24-add-dense-tool-resume-pty-probe/verification.md` | 512-turn、约 31 MB dense-tool，按键 p95 52.9 ms；采样窗口内首个 streamed marker 未出现，更不能据此声称已满足连续滚动。 |
| `2026-09-24-close-resume-responsiveness-watch/verification.md` | 已关闭的是有范围的合成探针；真实长文本、多子会话、resident/cursor 和不同终端仍需要证据。 |

## 优化顺序

先量化当前分段耗时和内容推进间隔；然后拆分实施 R1 的身份索引、R2 的 append plan、R4 的 clean/height-only 布局。它们能减少随历史增长的重复计算。R3 可单独做常数优化。运行时 [S1 exact commit](runtime-audit.md) 与同一 updates 日志有关，也应优先测量，但不要把所有存储改动合成一个大 change。

如果做完仍被全量解码限制，再讨论 checkpoint。Checkpoint 可减少反序列化和 fold，但若每次仍需校验全部前缀字节，I/O 仍是 Ω(B)。真正只读 suffix 需要可信前缀、失效与恢复契约，属于独立持久化设计，当前不建议先引入。

后续基准沿用 `docs/development.md` 中 `session_load_perf`、128/512-turn PTY 和 dense-tool fixtures，增加 2048-turn 与不同单条消息长度。固定 build profile、数据和 writer delay，分开测 warm/cold 与 correctness assertions；比较中位数、尾延迟、分配/RSS 和增长斜率，不用一次 debug 时间承诺生产加速倍数。
