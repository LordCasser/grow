# Resume 剩余瓶颈与下一轮方案

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

## 结论和证据边界

下一轮先做 **TUI 消息完成时的无效扫描与复制消除**，再做 **Timeline 解码的重复遍历消除**；**先筛选、后展开历史正文**列第三，主要服务 reconnect、rewind 和子会话展示。普通根会话投影已经很小，不应为它再造缓存或第二套恢复器。首次历史绘制的调度可以另作小范围实验，但不能用提高持续帧率代替算法优化。

本轮未实施、未编译、未新增 release 测量。数据来自 [P2 已归档验证](../2026-10-22-reuse-validated-descendant-recovery/verification.md)，重新处理其 `pty-log/attribution-summary.json` 与 `after-modern-{1,2,3}/*2048*-loading.json`。提取脚本与结果在本目录。九个热点源文件 SHA256 与 P2 `source-after.json` 一致。原环境是 macOS arm64、Homebrew Rust 1.99.1，release/default CLI features，warm page cache、新进程、每组 n=3。这里只解释该样本，不推断冷磁盘、图片密集、所有真实会话或其他硬件的绝对延迟。

边界沿用 [Timeline 编码与恢复契约](../../../specs/session-timeline/spec.md#requirement-timeline-wire-decoding-rejects-ambiguous-typed-fields)、[完整性契约](../../../specs/session-timeline/spec.md#requirement-stored-timeline-facts-validate-their-encoded-integrity)及 [有序历史恢复](../../../specs/client-surfaces/spec.md#requirement-resume-restores-ordered-communication-history)。保留全量校验、历史交付与就绪屏障，不提前释放普通输入；恢复期间仍只处理退出等既有控制。不引入 SQLite、checkpoint、跨恢复缓存或分页。

## 1. 时间实际花在哪里

以下为 **P2 after 的详细诊断模式**中位数，单位 ms；不是默认关闭诊断的绝对延迟。阶段嵌套、跨线程重叠，不能相加成总耗时。

| 阶段 | 普通 2048 轮 | 慢 writer 2048 轮 | 含义 |
| --- | ---: | ---: | --- |
| Timeline read/decode | 201.506 | 203.833 | 含 IO、JSON、hash、AppendState 索引、HistorySnapshot 构造；并非纯 JSON CPU |
| Timeline fold | 4.601 | — | 已非主要热点 |
| 全量历史 projection | 10.756 | 11.130 | 先构造所有 owned update，再交付 |
| replay | 235.802 | 237.599 | 包含 projection、转发和 gateway completion 等待，并非纯投影 |
| TUI apply_batch 合计 | 191.839 | 179.395 | 普通 133 批，每批最多 32 条 |
| message finish 合计 | 116.994 | 112.584 | 嵌套于消费，其中含下两项及正文复制 |
| 图片引用提取 | 92.675 | 91.060 | 无图片文本仍扫描；不等于 Markdown 全文解析 |
| Mermaid 检测 | 7.206 | 7.417 | 文本候选检测和必要解析 |
| frame render 合计 | 57.349 | 54.524 | 普通 7 帧，慢 writer 6 帧 |
| frame delivery 合计 | 0.580 | 242.122 | 包含 writer 等待，不是纯 write |
| 其中 writer wait | 0.095 | 241.647 | 慢输出的主要额外成本 |
| 其中实际 write | 0.327 | 0.298 | 只看它会漏掉慢输出瓶颈 |
| replay finalize | 3.827 | 2.990 | P2 已消除这里的根重复读取 |

普通 2048 的首个 sequence marker 可见为 421.532 ms，末 marker 可见为 548.539 ms；慢 writer 分别 418.496 / 664.090 ms。**首个检测到的 marker 是 viewport 中某个历史 marker，不保证是历史第一条消息。** 不能把它叫精确的“首通知”或与 RPC first 时间直接相减；RPC/PTY 的 fixture、时钟及数量不同。稳定 prompt 还含固定 500 ms hold，也不是 ready 时间。

从同一个 instrumentation 时钟推导，普通三次的全量投影结束于 load 开始后 217.03–232.53 ms；随后首个 batch 几乎立即开始，但下一帧 present 开始仍晚 **96.57 / 104.66 / 112.32 ms**，该帧 present 自身另需约 30–34 ms。首个 batch 只是交付代理，尚无确切的首历史 send/consume/paint 标记，不能把整段间隔都归为 timer 睡眠。源码 `event_loop.rs::automatic_draw_interval` 保留 loading 最低 100 ms 绘制间隔，和现象吻合：优化后的数据若仍错过同一个绘制时点，首屏可能看不出变化。

三次最大 batch 仅 2.007–2.247 ms，p95 1.516–1.691 ms。目前没有扩大 32 条批次的证据。正常 ACP 走 `pager/src/acp/spawn.rs` 的 direct typed channel；Grow extension 仍有 JSON 编解码，RPC/leader 路径也另有序列化。旧 no-draw 的 `decode_project_ms` 是**磁盘 Timeline 恢复和全投影**，不是前端 ACP JSON 解码。不能把它重复算入 TUI。

## 2. 重复工作及最小改造

### 优先级 1：TUI 完成消息时，去掉无效扫描和整段复制

证据：`scrollback/blocks/agent.rs::finish` 先 `content.text()` 复制整段正文，再做图片引用提取；复制位于 image_refs timer 外。`pager-render/src/prompt_images.rs::extract_image_refs` 无条件跑两组 regex：Markdown 图片与 Unix/Windows/UNC 裸路径。样本 2048 条 assistant、约 64 MiB 正文，即使无图片仍产生约 93 ms 提取成本。

最小方案：沿现有 source 借用访问正文，避免为了检测而 clone；在现有 extractor 内做**保守的否定筛选**，没有 `![` 就不运行 Markdown 图片 regex，没有 `/`、`\\` 就不运行裸路径 regex。保留原 regex、匹配顺序、去重、alt 和完成时的文件观察行为。复用同一批图片字节的首次尺寸探测结果，去掉 `is_decodable_image` 后的重复 `decode_image_dimensions`；这里调用的是 header/dimension probe，不能宣传为省两次完整像素解码。Mermaid 的小写整段临时字符串也可用无分配候选检查替代，但保留 entity/围栏语义，不趁机改完整 Markdown 解析策略。

对抗性约束：不能仅按 `![` 判断“无图”，裸绝对路径也受支持；URL、代码和斜杠密集文本常走原扫描，新增筛选可能不赚反亏，必须独立测此负载。不能把图片探测推迟到用户点击，否则文件在 completion 后改变会影响历史展示；不缓存跨消息文件结果，不更改 raw/pretty copy、tab/占位规范化、source map、布局 generation。只借用不可变正文，不跨 RefCell 可变操作持有借用。

收益边界：93 ms 是该 fixture 中整个 image_refs 阶段的可优化预算，约占消费阶段 **48%**，不是保证可省 93 ms；仍有一次廉价文本扫描、候选图片工作及其他消费。正文复制和 Mermaid 的收益需另计实测。服务工作减少不会等比例转化为首/末屏变化，图片密集会话尚无计时结论。先做它是因为证据直接、影响局部、验证成本低，不是因为它能缩短交付前的 202 ms。

### 优先级 2：Timeline 读取保留一次完整校验，减少重复 JSON 遍历

证据：`history/mod.rs::decode_record` 对 Domain 依次执行：外层 Version、Record(RawValue)、FactHeader(RawValue)、内层 Version、typed TimelineEvent。是 **五次范围相互重叠的 parser 遍历**，不是构造五份对象；RawValue 借用仍需跳过并检查 JSON 语法。另有原始 fact checksum、event digest 和完整物理前缀 hash。`jsonl/ledger.rs::read_journal_prefix_data` 的约 202 ms 还包含索引、来源记录与 IO，不能全部归给 parser。

最小方案：先在该次优化的 release baseline 中把 read/decode 内的 decode、hash/index/source 构造作有界归因，再以标准 serde 的 header/visitor 组织合并冗余版本探测与 typed 解码。优先减少两个 Version 的全 payload 扫描，不先改磁盘格式或手写通用 JSON parser。外层 envelope 和内层 TimelineEvent 分开验证；保留 RawValue 原始字节校验及所有 hash 的各自语义。TimelineEvent 的 flatten + 带 tag/content 的 enum 还有中间缓冲嫌疑，但当前数据没有证明其独占成本，不立刻为全部事件手写另一套反序列化器。

对抗性约束：未来版本且 payload 是未知形状时，仍先准确报告 persisted/current version；任意合法字段顺序、重复/未知 typed 字段拒绝、JSON Value 入口继续成立。不能先构造 Value 后 last-wins，也不能为省遍历绕过 checksum、exact payload 身份冲突、prefix hash、完整行/部分尾片、领域 fold 或 artifact 验证。若合并版本探测需要大范围重复类型/定制 parser，收益不足时应放弃该子方案，而不是扩大 change。

收益边界：仍是 O(历史字节 + 事件数)。假设 read/decode 总阶段实测降低 25% / 50%，则交付前分别省约 **50 / 101 ms**；这是敏感性计算，不是五次变一次的倍数预测。完整性 hash 和最终 typed 对象仍要产生，真实收益必须测。actor spawn 在普通 cold 路径 replay 后，不能把其约 36 ms 当作首历史前热点。

### 优先级 3：先按来源筛选，再展开 owned 历史正文

证据：`history/projector.rs::project` 先展开全量 updates、clone canonical 文本、为每行序列化 cursor，再 rewind 过滤和 cursor 查找；查找还重新解析每行 cursor。`storage/descendant_recovery.rs` 同样先 `into_prepare` 所有正文，再筛选 lifecycle/control。这里有明确浪费，但普通 full root 投影只约 11 ms。

最小方案：同一已校验截点中，先用轻量来源位置/part 做 rewind、cursor 和消费类别选择，再调用**同一个 projector**展开选中消息；cursor 边界直接比较结构化位置，不先编码再解析。不要建立第二份持久投影或另一套快速语义。只有测得 full-root owned Vec / gateway 排队的峰值内存或等待确为问题，再把选中结果按有界批次顺序 materialize/交付；它不是分页，ready 仍等待所有必需历史交付完成。

对抗性约束：全量校验仍先完成，不能因“筛选不显示”忽略损坏正文。历史 anchor 与 admission 的物理位置、canonical 多 part/complete、quarantine、省略的 candidate、consumed echo、rewind 留存以及 tool terminal 合并必须一致。不能看到后续 rewind 前就把会被删除的前段送给用户。cursor 只匹配相同 session/version/cut/hash 的完整有效行，失效仍 full replay。执行恢复和 child ownership/Seed/Result 验证不被展示选择替代。

收益边界：普通 full root 即使消掉整段预构造，首通知前上限也只是约 **11 ms**，并且展开成本会移到交付中；不会解决 202 ms 的读解码。reconnect、rewind、child lifecycle 可能省大量“构造后丢弃”的正文和瞬时分配，但尚无分项计时，不能外推 P2 的 46% 子树提升。gateway 当前入队所有消息再等待 completion；若后续做有界交付，只限制 replay，不能全局改 live/fire-and-forget 通道，避免等待循环、取消悬挂和顺序变化。没有内存数据不声称峰值下降比例。

## 3. 首屏调度和慢输出：配套门槛，不扩成框架

首个历史实际消费/send/present 缺精确关联，应在下一项基线中补**每次恢复一次**的轻量标记，并把 `history.validate/load_notices` 放入准确计时范围；当前 evidence timer 在两者之后才开始。无需再造独立诊断框架。

先保持 100 ms 持续 cadence 和 writer 合并/ACK。若优化 1/2 后仍有一整档首屏等待，再单独实验“首次历史有效变更申请一次绘制”，复用既有 draw 调度，不按每条消息绘制。理论只可能减少接近一个 cadence 的可见等待，不能保证恰好省 100 ms；慢 writer 下新增一帧可能增加输出等待。必须用 slow writer 对照拒绝末屏回退，不能用首屏成绩覆盖它。不得丢弃历史 mutation、跳过原生终端 scrollback 或以跳到末屏代替快速滚动。

## 4. 后续实施的共同验收

每项独立 change、同 source/harness 的 release AB/BA 对照。分别记录完整读校验、第一条真实历史 send/consume、首历史 paint、末历史 paint、SessionLoaded/输入释放，以及 CPU、正文复制字节和峰值队列/内存；保留默认关闭诊断和详细诊断两个口径。不是把跨线程嵌套时长相加，也不是要求现在新增一整套 bench 框架。

性能负载至少包括现有现代 128/512/2048、慢 writer；按改动补斜杠/代码密集文本、真实/缺失/损坏图片、Mermaid/entity、reconnect/rewind、根子孙树。正确性沿现有全 family/prefix oracle 覆盖用户、assistant/reasoning、tool/update、权限、plan/status、receipt/control、图片与子会话消息，比较原正文、身份、历史顺序、raw/pretty copy、80/120 列布局、resize/滚动及末屏锚点。校验格式错误、重排/重复字段、未来版本、checksum、损坏与半尾、前缀变化、ACK/写入失败、退出/取消和重新恢复。加载前不放行普通输入，仍验证 Ctrl+D；完成相关范围后归档并删除专用编译产物。

本轮仅分析，未重新执行上述运行测试，也不以旧测试代替下一项实现的回归。前一项专用 `target/descendant-recovery-release` 已不存在；本轮未产生 Cargo 残留。
