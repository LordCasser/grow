# Grow 底层算法优化审计

能从算法上继续优化。当前最值得处理的，是随着历史增长反复扫描旧数据的路径，以及只改少量内容却重新构建完整派生结果的路径。单纯增加线程、提高 batch 或加 spinner，不能消除这些成本。

本次保存了 resume 的分析，并检查 Grow 其他主要子系统。共记录 **17 个独立候选**，其中 R4 与 U1 指向同一个布局问题。这里只完成分析和拆项，没有修改生产代码，也没有得到新的性能测量结果。基线为 2026-10-21、HEAD `a1d1c2965075c87645eab22adce6d48b19410c59`。

用户体验目标保留为：短加载优先速度；长加载让真实历史连续快速推进，避免中途停住。首次历史出现、历史推进间隔、末条历史到可操作的等待，与按键响应分别测量。已有按键延迟测试不能证明历史连续推进。

## 候选与实施顺序

A 表示优先进入基准和最小实现；B 表示有明确机会，但受工作负载或实现复杂度限制；C 表示先测量。优先级按触发频率、增长维度和语义风险排列，不是已测耗时排行榜。各行变量在明细中定义；O(1) 为索引 lookup 的平均复杂度，不含正文处理。

| 顺序 | 候选 | 当前重复工作 | 最小改进方向 | 证据 |
| --- | --- | --- | --- | --- |
| A | R1 Timeline admission | 每个响应查旧 events、clone/检查 Surface；O(R×E + ΣS_i) | 完成 attempt / 已接纳 identity 索引；只读 quarantine 计数；再评估增量 Surface 摘要 | [恢复分析](resume-audit.md) |
| A | R2 恢复 projection | 规划后每个 projection 再扫描 U 条 updates，O(P×U) | 直接返回缺失 append plan，同 revision 复用 branch 分析 | [恢复分析](resume-audit.md) |
| A | U1 / R4 Pager 布局 | clean frame O(N) 求和，局部测量重建全 offsets，dirty×prompt 扫描 | 复用已有前缀位置；区分高度和结构变化；按索引修正 descriptors | [UI 分析](ui-audit.md) |
| A | S1 exact commit | 每次重读/解析整份 updates，追加后再次重查 | writer 所有者维护已验证身份索引与 suffix，保留 exact conflict 和 sync barrier | [存储分析](runtime-audit.md) |
| B | R3 JSONL / replay JSON | RawValue→完整 Value→typed；typed notification 再转 JSON 重解析 | 去掉完整中间树与转发往返；仍是 O(B) | [恢复分析](resume-audit.md) |
| B | S2 搜索更新 | debounce 后仍全量 Timeline fold，hash 相同也已付出成本 | 区分 metadata/body；按 accepted event 维护可重建投影 | [存储分析](runtime-audit.md) |
| B | C1 图片预算 | K 个替换各自线性查找 SurfaceId，O(N×K) | 一次建立 SurfaceId→index，平均 O(N+K) | [上下文分析](context-audit.md) |
| B | C3 prompt cache key | 每次请求反向扫描全部 events 找 rewind | 随已接受事件维护最后 rewind anchor | [上下文分析](context-audit.md) |
| B | T2 多 hunk patch | 每个 hunk 重建两份完整文件行表，O(K×(B+C)) | 每次文件查询各建一份行表，共用切片 | [工具分析](tools-audit.md) |
| B | T1 初始 fsnotify 选择 | 全收集/排序目录后才按 watch budget 截断 | 有界 top-B，保留浅层优先和同深度顺序；仍需遍历目录 | [工具分析](tools-audit.md) |
| B | T3 workspace 列表 | 普通条目也执行两次 metadata syscall | 复用非 symlink 的 metadata，只为链接跟随目标 | [工具分析](tools-audit.md) |
| B | S3 session 列表 | 全量转换、时间 format/parse、排序后截 L | 先按 typed key 去重/选择，再转换；先做简单部分 | [存储分析](runtime-audit.md) |
| C | C2 损坏历史修复 | K 次 Vec splice/remove 移动后缀，最坏 O(N×K) | 有序编辑计划一次 rebuild；健康历史不触发该成本 | [上下文分析](context-audit.md) |
| C | C4 请求前完整性检查 | 健康 Surface 额外 clone 后丢弃，再 clone 请求 | 复用候选所有权，保留 durable repair 先于 request | [上下文分析](context-audit.md) |
| C | U2 session picker | 每帧过滤并构造全部 S 项字符串，视口只画 V 项 | 按实际列表规模选择可见项构造或派生数据复用 | [UI 分析](ui-audit.md) |
| C | U3 dashboard 子项 | 排序全部 M 个 child 只显示 8 个 | 稳定 top-8，保留相等键来源顺序与总数 | [UI 分析](ui-audit.md) |
| C | T4 工具 manifest | 每 Step 深 clone 相对稳定的 schema/description | 先测复制量，再考虑不可变 registry snapshot 共享 | [工具分析](tools-audit.md) |

## 覆盖到哪里

| 子系统 | 深度与结论 |
| --- | --- |
| 冷恢复、Timeline、投影和 replay | 核对生产入口、validator、fold、planner、存储调用方和历史性能记录；R1–R3，布局归 U1。 |
| 存储写入、搜索、session list | 核对 exact append、搜索调度/投影、summary 排序与分页入口；S1–S3。 |
| ChatState / request / compaction | 核对请求构建、图片预算、repair、rewind key、tool exchange；C1–C4。 |
| Sampler / provider encoding / token estimation | 核对主要 stream/retry/encoding/prune 路径；没确认新增高价值候选，已有预算或线性处理，不强行添加缓存。 |
| Pager / markdown / render / picker / dashboard | 核对布局、replay deferred、过滤、completion 与行构造；U1–U3。 |
| Tools / workspace / hunk / fsnotify | 核对生产调用、大小/数量上限、metadata 与 watch 选择；T1–T4。 |
| Memory / codebase graph | 核对候选数、去重/排序/MMR、文件发现与 parser cache；默认候选较小，发现新文件需要枚举，未新增候选。 |
| Workflow / prompt queue / coordination | 核对 journal seq 索引、恢复上限、队列容器与编辑、inquiry 身份和清理入口；没有足够负载证据支持新增索引。 |
| Config / MCP / hooks | 核对 watcher debounce、dispatcher 身份合并和 catalog 任务替换、compiled matcher；没有新增候选。网络与插件启动时延未测。 |

“全面”按主要子系统和数据增长维度展开，不是逐行穷举。没有深入第三方 C/SQLite/向量库、sandbox/OS 隔离、所有 provider SDK、所有终端/图片编码、release/build 工具链，也未覆盖每个 UI 控件。没有通过读取用户真实 session 估算规模；各项收益需要后续合成样例和经授权的真实负载证据。

## 对抗性复核后的取舍

- 已有 bulk fold、lazy viewport、deferred Markdown、replay repaint 限频，不能重复当作未做方案。以当前源码为准，旧性能记录仅用于认识可能的主导成本。
- Timeline 正常 append 已有 prefix hash 增量缓存；不成立“所有追加都会扫描全历史”。S1 限定在 exact updates commit。
- `get_turn_hunks` 存在 ID×全 hunk 查找结构，但未找到生产调用，不计入 17 个候选。notification schema catalog 的生成也未确认生产调用。
- Prompt-history 200、suggestion 50、默认 memory 小候选、文件 diff 1 MiB 等上限影响价值排序。上限不会消除重复计算，但不能拿不存在的无限规模论证优先级。
- hash/identity 相同不等于完整 payload 一致，更不等于持久化成功。exact conflict、file/directory sync、writer 所有权和完整恢复验证必须保留。
- 缓存只归已有 owner，失效与 accepted event/revision 绑定；不引入全局万能缓存。被 rewind 隐藏的 physical event 仍占用身份；搜索的 selected-branch transcript 不等于当前 Surface。
- top-k 必须先维持原过滤、去重和 stable tie-break。先截断再去重、只看前八个 child、随意改同深度 watch 顺序都会改变结果。
- 没有发现需要通过本次文档修改主规范的已确认实现偏差。连续历史推进是待设计/验证的体验要求，不能把现有输入响应测试解释为已满足。

## 后续最小闭环

先为 R1/R2/U1/S1 记录操作数、读取字节和分段耗时，再每项单开 change。R1 身份索引、R2 计划复用、U1 clean-frame 总高度分别可以作为小切口；S1 必须先证明索引覆盖所有写入和失效路径。T2 则适合作为独立的小型算法改动，不需要牵动恢复架构。

每个 change 都要同时有语义差分和规模曲线：相同 fixture 的旧/新输出及错误行为相同，128/512/2048 或相应规模下观察工作量增长。端到端 resume 同时关注总时间与可见内容最大停顿。具体边界和现有测试见各分报告；本次验证记录见 [verification.md](verification.md)。
