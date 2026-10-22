# Resume 优化：本轮范围与三个独立 changes

2026-10-22 状态：本轮不做 SQLite，也不做 ready 与历史分页/回放分离。完整校验、从头正序回放、所有历史加载完才开放普通输入；loading 保留 Ctrl+D、resize 和现有绘制节奏。C1/C2 已实施、验证并独立归档；C3 的完整合并方案已完成实施、正确性和正式配对验证，专用构建已清理。最终结果见 [C3 验证](../2026-10-22-unify-timeline-history-storage/verification.md)，按流程完成归档收尾。

## 顺序与边界

| 顺序 | Change | 内容与主要代价 |
| --- | --- | --- |
| 1 | [streamline-resume-codec-and-projections](../2026-10-21-streamline-resume-codec-and-projections/proposal.md) | 移除完整 Value 中转、临时容器复制和 typed/JSON 往返；格式不变，直接 typed 解码拒绝重复字段的差异有明确 delta。 |
| 2 | [reuse-validated-timeline-prefix](../2026-10-21-reuse-validated-timeline-prefix/proposal.md) | 同一 lease/adapter 内复用初次读取产生的现有 LedgerPrefix；保留文件变更验证，增加一次读取中的 hash 工作。 |
| 3 | `unify-timeline-history-storage`（C3） | 已按完整合并方案实施：补齐 typed 历史依据，统一物理日志并迁移 schema、cursor/frontier 和读者；最终验证与正式配对数据已完成，见相邻 C3 verification/paired-analysis。 |

三项分别测量和验收；前两项可独立落地，不等待存储迁移。第三项复用前两项的解码和 prefix 能力，不保留马上会淘汰的第二套实现。理论收益来自去重复工作，不改变复杂度下界：仍需 O(全历史事件、必要正文和完整输出)。

目标只保留现有 owner：ChatState/SessionActor 决定事实，persistence 持有文件和 durable ACK；History/Surface 引用同一正文。普通正文内联 Timeline，大证据继续用已有 artifacts。summary、Sideband、rewind 的职责不变。没有 SQLite、Git DB、新 blob/pack、压缩、检查点、Merkle 树、新持久 cache、通用事件服务或分页协议。

## 对先前方案的收紧

1. **不再设计跨 actor 的验证证明。** 源码确认 `load_session_for_write_without_updates` 先取 lease，storage adapter 随后进入 persistence actor；在其现有 prefix 状态内复用即可。相同 seq/文件大小不能代替验证。
2. **不把 updates 当成已经可删除的缓存。** `DisplayProjectionExpectation` 只有 identity/revision/digest，某些 Workflow/Control 和用户 display 信息仍需原记录。删除前必须补齐最小 typed 来源，禁止直接搬入 opaque ACP JSON。
3. **不为每个历史查询添加永久增量索引。** 同次规划只计算一次 branch，借用既有正文；只有测量证明剩余重复查询显著时再另议。
4. **不先放宽 candidate anchor 的 durable 次数。** 沿原先提交顺序，response admission 的必要展示引用在其唯一记录闭合；避免把同步优化混入正文去重。
5. **不以删 cache 之名丢失完整性检测。** 新格式的事实编码 checksum 接替旧 projection 正文/digest 比较的修改检测职责；恢复校验原字节，不再重序列化整份 response。
6. **不把 UI 故障当作第二个磁盘提交失败。** durable admission 后交付失败保留 canonical 事实；不重采样/重发工具。原 preview 预算失败、独立事实持久失败与执行边界仍保留。

## 周边机制与对抗性验收

| 风险 | 必须验证；不能采用的捷径 |
| --- | --- |
| schema/窄解码 | 非法版本、未知/重复字段、完整坏行、大小限制和尾片；不能靠宽松解析得到性能。 |
| prefix 复用 | 同长度改写、替换、truncate、读中修改、路线补写、验证失败、跨 lease、只读；不能只给旧 hash 贴新 stamp。 |
| durable ACK | file/directory sync 失败、完整可读但未确认、重复 exact、异 payload；不得以可读代替已同步。 |
| 历史唯一内容 | 用户 display/hide/skill、工具结果、权限/Hook/图片恢复、Workflow/Control/Goal、通信和 child；不能仅验证大段 assistant 文本。 |
| 排序与并发 | anchor→独立事件→admission、resident 截点后追加、半个 response cursor、live 去重；不能以 seq 高水位推断较早 anchor 已展示。 |
| 分支与执行 | compaction 不删 transcript、rewind 不复活、fork/import 不继承执行资格、挂起子树与待交付结果保持；观察不得取写者身份。 |
| 运行成本转移 | 正常 turn 的 write/fsync、提交延迟、锁范围、内存；不能用写放大换恢复成绩。 |
| TUI | full/minimal 正序、连续快速加载、普通输入忽略、Ctrl+D 与重新恢复；不提前 ready，不新增后台历史阶段。 |

这些边界分别落实到三个 design/tasks。源代码核对已识别上述实际缺口；updates 全族无损覆盖是 C3 的首个验收门槛，C3 已完成相应迁移与最终验证；逐场景范围和未穷举边界以 C3 scenario-coverage 为准。

## 实施前理论预算（实际结果见 C3 验证）

来源：R6 modern-2048 run 3，load RPC 9861.964 ms、首条 CountingClient 历史 6691.948 ms。不是 TUI 首次绘制，也不是 release。

| 成本 | 时间 | 可推断内容 |
| --- | ---: | --- |
| Timeline read/decode | 931.047 ms | 第 1 项能减少其中重复解析，必要读取/解码仍在。 |
| 主 fold | 15.775 ms | 不是当前主要热点；本轮不做 fold checkpoint。 |
| forward updates | 1575.800 ms | 第 1 项去掉其中 JSON 往返，通知准备/传输仍有成本；不把全部当节省。 |
| canonical + updates decode + response raw reconcile + raw snapshot | 3720.692 ms | 第 3 项重点替换的阶段，占总 RPC 37.73%；必要 checksum/typed History 不是免费。 |
| actor call | 1744.030 ms | 第 2 项所处父阶段，包含其他启动工作；没有 prefix 独占计时，不能宣称全省。 |

仅把第 3 项四个阶段全部置零且替代工作免费，剩余 6141.272 ms，理想上限约 1.606×。再极端地把整个 actor call 置零，剩余 4397.242 ms、2.243×，这只是故意乐观的预算，不是两项的可实现承诺。第 1/2/3 项触及的工作重叠，必须依次测增量，不能叠加不同基线的百分比。此前 1.5–2× 不作为验收承诺；目前没有 release 加速预测。

现代 2048 轮样本 Timeline/updates 约 71.03/77.14 MB；重复 canonical 正文一份 64 MiB。仅消除这一份正文相当于两日志合计约 45% 的粗略空间，新增 typed 来源/checksum 会回补，其他 session 文件尚未计入，不等于完整目录必减 45%。不计未实施压缩收益。

首条历史 6.69 s 由 load_light 4.252 s、raw snapshot 0.026 s、reconcile 0.718 s、同步全量入队 1.576 s 及约 0.120 s 未归因余量组成。这里无重叠；load_light 内部阶段不能再相加。seeded-route 的 writer 初始化在初次 replay 后，第 2 项主要影响 ready，不能拿来解释首条历史。

详细同轮数据见 [analysis](analysis.md)、[breakdown](debug-first-history-breakdown.json) 和 [R6 验证](../2026-10-21-reuse-resume-canonical-projections/verification.md)。

## 共用基准与通过条件

- 使用仓库固定工具链、相同 release profile 与持久同步设置，编译和夹具生成不进入计时；冷进程/温热页缓存明确标注，至少三次并保留逐轮原始记录，不把 debug 外推 release。
- 现代 admission 长会话 128/512/2048 × 32 KiB，加 512 × 4 KiB；另补 compaction/rewind、独立事件夹 anchor、挂起 child/待交付结果和 Workflow/Control/user display 缺口。
- 第 1/2 项沿用 missing projection repair 样本；第 3 项不再有该故障类型，改测缺必要事实/引用、checksum 冲突及 ACK 不明，不能声称“repair 归零”是同工作量加速。
- 新旧格式使用相同业务历史和展示 oracle，分别报告数据规模；不要求物理字节相同。比较首条通知、PTY 首条/末条可见、完整 load/SessionLoaded、峰值内存、全目录体积及正常 durable commit 延迟/写入次数。
- 本轮不改变 ready 定义。性能改进必须同时通过恢复/持久化/历史语义断言；没有可测收益时记录并删去无收益的额外机制，不扩大范围补成绩。

初次 release 构建中断的记录仅作历史追溯。后续已固定工具链完成基线、C1/C2 的前后 release 矩阵与正确性验证，并独立归档两项。C3 已完成确认范围的实施、最终 correctness 和正式配对测量；实施前范围复审见 C3 的 `scope-review.md`，最终证据见 [verification](../2026-10-22-unify-timeline-history-storage/verification.md)。基线不作为优化后成绩，早期范围缩小建议未采用。专用构建已清理，归档前 strict 18/18 通过。检查点、SQLite 和 ready/历史分页分离未实施。
