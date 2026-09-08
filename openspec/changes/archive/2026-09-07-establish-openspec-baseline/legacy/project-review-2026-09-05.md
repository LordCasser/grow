# Grow 项目架构审查 · 2026-09-05

这轮先从文档建立架构模型，再沿代码核对事实。结论是：核心分层已有清晰的所有权约束，但部分路径仍有遗漏的载体、重复状态和全量历史处理。发现 5 项可定位的问题，另记录 4 组需要独立处理的架构与工程债务。后续先完成 F1–F5 修复，再按用户要求分别清理 D1–D4。下面保留两轮改动和原审查依据。

**修复记录**

| 项目 | 已落地的边界 | 回归证据 |
| --- | --- | --- |
| F1 图片预算 | User 与 ToolResult 共用图片枚举，补齐 ToolResult token 估算；超限请求在 Sampler 编码完成后、发送前失败，禁止无效重试；portable 投影保留预算允许的普通图片，仍移除 native tool 协议与 reasoning 状态 | 三种 provider wire 的 live 图片序列验证实际请求大小、最新图片可见性、ImageBudget 事件与原始 Timeline 不变；混合载体逐出和 wire 大小边界测试 |
| F2 watcher | 删除独立 dirty bool，仅从 ArcSwap 集合判断是否有待处理文件 | 确定性暂停 RCU 生产者、取走旧集合、继续生产者，验证新文件仍然可见 |
| F3 向量身份 | 缓存绑定无凭据的端点、模型和维度；身份变化只重建向量，保留 Markdown/FTS；向量操作在同一事务内检查身份，拒绝旧 handle 的迟到写入 | 同维模型/端点切换、凭据轮换、旧 handle 写入拒绝、FTS-only 打开不破坏向量；本机 HTTP fixture 验证无 watcher、无文件变化时仍补齐新模型向量 |
| F4 协调恢复 | `InquiryEvent` 写入既有 Timeline Observation；开始、接收、审批、结果及恢复终态等待 ACK；UiNotice 作为派生显示；来源查询和接收恢复均不再读取 UI replay | 临时会话删除 replay 后仍能查询/重用来源终态；接收侧从 Timeline 恢复，仅关闭未完成询问，重复恢复不增加事件或调用模型 |
| F5 追加成本 | 普通消息和 Memory 注入使用既有 prepare/ACK/accept；prompt 坐标在 Timeline 内增量派生，不再每次 commit 扫描全部事件 | prompt 坐标缺口、rewind、分支坐标复用和重新加载；既有提交、恢复、压缩回归 |

F1–F5 修复完成时的验证：`chat-state` 459、`memory` 296、`sampler` 215、`sampling-types` 264、`workflow` 65、`shell` 3,660 个单元测试通过，合计 **4,959 passed / 0 failed / 3 ignored**。三个跳过项来自 Shell 既有测试。没有访问真实模型服务；新增 HTTP 回归只使用本机 fixture。

```sh
CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test \
  -p chat-state -p memory -p sampler -p sampling-types -p workflow -p shell \
  --lib --offline --quiet -- --test-threads=4
```

格式检查与 `git diff --check` 通过。修复前已保存现有源码快照，核对后没有发现修复清单之外的源码变化；原有未提交工作保留。未执行跨平台、完整 TUI 实机或外部服务回归。

D1–D4 在第一轮作为独立待办保留，后续清理结果见下表。F5 消除了报告指出的全量 clone 和 prompt 扫描，不代表所有 Timeline 操作都变为常数时间。旧版本仅保存在 UI replay 的询问不回填为新的 Timeline 权威事实。

**债务清理记录**

| 项目 | 落地边界 | 验证重点 |
| --- | --- | --- |
| D1 因果证据 | ChatState 冻结来源与图片逐出位置；Sampler 在 HTTP emission 前等待实际 wire body 的 artifact/Timeline ACK，保存解码前原始响应，重试决定单独等待 ACK；Sideband 接入同一存储。native state 仍只在 runtime 使用 | 三 backend 的请求确认阻塞；响应/重试 ACK 成功、失败、丢失和取消；Sideband 失败后禁止下一 attempt；非 UTF-8 正文导出和缺失/篡改 artifact 拒绝加载 |
| D1 存储成本 | body 按 64 KiB 固定块寻址，Timeline 只保存有序引用；同一前缀复用已有块，恢复逐块限长校验，不需要再拼出整份 body | 两次增长请求只新增尾块，按引用重组后字节完全一致；64 MiB 响应捕获上限，超限保留有界前缀并停止 |
| D2 prefix 校验 | fresh writer 与 stamp 变化时逐行读取、逐事件验证、增量 hash；继续使用原有锁、compare-and-append 和缓存 | 精确 committed prefix hash、CRLF/空白字节、尾片、短读、空行与超长记录；既有内部损坏和幂等追加测试 |
| D3 文档漂移 | schema 统一为 v25；明确 Sampling/Agent desired slot、边界资格、superseded 与 Pager 并行发送/重连相关性；补齐 continuation epoch 对 cache key 的影响 | 文档 schema header 对代码常量的校验；现有边界/重连测试 |
| D4 公共 PR CI | 新增无路径过滤的 `Core regression`，执行八个核心/展示 crate 的现有单元测试并构建 CLI，保留专项跨平台 workflow | YAML 解析、PR 触发入口检查和本地核心回归/CLI 构建；未修改远端 branch protection |

实现入口：[Sampler 证据边界](/Users/lordcasser/workspace/projects/grow/crates/codegen/sampler/src/audit.rs)、[Timeline artifact 存储](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/sampling_evidence.rs)、[公共 PR workflow](/Users/lordcasser/workspace/projects/grow/.github/workflows/core-regression.yml)。请求证据不是 HTTP 连接重放包：请求记录保留去除凭据/query 的 route，不保存认证 header；进程退出前未确认的响应按 interrupted/unknown 处理。存储错误保持不可重试；在证据确认之前取消、未进入 HTTP execute 的 attempt，已捕获的 Goal lease 按零用量结算。

债务清理后的完整核心回归：`chat-state` 460、`memory` 296、`sampler` 218、`sampling-types` 264、`workflow` 65、`shell` 3,665，通过 **4,968 / 失败 0 / 既有跳过 3**。使用 `--locked --offline`，新增 HTTP fixture 仅监听 loopback；`cargo build --locked --offline -p cli --bin grow`、格式检查、`git diff --check` 和 workflow YAML 检查通过。此前旧测试要求“取消后仍先发 Retrying”；现在取消不会提交或显示未生效的重试决定，测试已改为检查直接取消且 retry count 不增长。

D2 消除整份原始 JSONL 副本和中间事件数组，验证用 Timeline fold 仍随历史增长。新增 CI 的 Linux runner 尚未在本地模拟；验证不会用 macOS 结果冒充跨平台运行结果。

以下发现与复现数字描述的是**修复前基线**。

基线是 `09c68b80` 加当前工作区的 51 个未提交文件，包含正在开发的 native continuation 改动。对源码与 Markdown 建立快照后，到完成复现时没有检测到其他修改。因此本报告描述当前工作区，不能直接当作已发布版本的缺陷清单。

**架构理解与审查范围**

主要依据是根 README、Agent README、Shell README，以及 `agent-core-timeline`、`behavior-state-overview`、`input-routing`、`goal-continuation`、`workflow-workspace`、`local-coordination`、`compaction-pre-prune`、`session-robustness-repair`、Pager 架构和用户文档。历史修复文档只作为设计意图和验证记录，结论以当前代码为准。

正常执行链路可以拆成六步：

1. TUI、headless、ACP 把输入交给 Shell；真实用户输入先经过 durable admission 与 Hook。
2. Shell 用唯一 foreground 和 FIFO 决定何时启动 Turn，Turn 捕获 Behavior。
3. ChatState actor 从 Timeline 的 Surface 组装请求，处理上下文投影与压力估算。
4. Sampler 负责协议、流校验、传输、重试；完整响应交回会话层接受。
5. 工具经冻结参数、能力交集和一次性 permit 执行，结果沿 Timeline 提交。
6. Turn 终结后释放 foreground，再处理用户队列、durable notification 和 Goal continuation。Workflow 与 child session 通过各自生命周期和精确引用连接。

| 边界 | 本轮核查 | 判断 |
| --- | --- | --- |
| Timeline / ChatState / storage | 事件提交、消息接受、请求组装、图片容量、历史遍历、JSONL prefix 校验 | 最值得优先处理的运行正确性和性能问题集中在这里 |
| Shell 控制与工具授权 | admission、idle arbitration、控制切换、permit 消费、结果入账 | 已存在明确边界；不建议以这次审查为由拆出新的通用 actor |
| Workflow | Definition/Run 冻结契约、journal pending/completed、host operation identity、恢复与预算对账 | 抽查路径有对应机制；没有把初步怀疑列成缺陷 |
| 本机协调 | 源端查询、接收审计、恢复读取路径 | 仍存在 UI replay 参与恢复的第二条事实路径 |
| Memory | watcher、增量索引、embedding cache identity、向量写入和查询 | 找到两个独立正确性问题 |
| Pager / 工程流程 | 文档、协调事件投影、控制语义、仓库内 GitHub workflows | 核查了事实投影接口与测试入口，未做完整 TUI 实机回归 |

这是架构驱动的关键路径审查，不是逐行穷尽全部源码。没有完整审计 vendored 第三方代码、跨平台 sandbox 内核实现、所有插件与 MCP 运行路径，也没有调用真实模型端点。Atlas 的一次符号查询返回了与当前源码不一致的行号，因此所有引用和缺陷判断都重新用工作区源码核对，没有把其局部查询当作全仓证明。

**F1 · [P2] ToolResult 图片没有进入统一容量处理，长图片会话可以越过请求预算**

触发条件：同一路由的 live continuation 中，连续读取多张图片，图片作为 `ToolResult.images` 保留。

事实链路：

- [`tool/result.rs:439`](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/tool/result.rs:439) 将 `read_file` 的图片放进 ToolResult，而不是 User。
- [`state.rs:141`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/actor/state.rs:141) 对 ToolResult 只估算 `content`，没有计算其图片。
- [`request_builder.rs:289`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/actor/request_builder.rs:289) 的计数和 [`request_builder.rs:395`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/actor/request_builder.rs:395) 的逐出候选只遍历 User。
- 即使测得 body 已超限，逐出仍可以是 0，随后正常返回请求；仅有 ToolResult 图片时，ImageBudget 事件也不会发出。

独立程序通过当前 ChatState 公共 API，先组装初始请求，再逐次接受 20 组 native assistant call 与带图 ToolResult，最后执行真实 Chat Completions wire 转换。每张图片载荷约 3 MiB，结果为：

```text
wire_bytes=62920527, native_spans=20
surface_tokens=83, request_tokens=733
remaining_images=20, image_budget_events=0
```

这里只用构造的 base64 字符串测试计量和投影，没有向端点发送图片。wire 本身已约 60 MiB，仍包含全部图片；token 低估会削弱自动压缩触发，byte 路径也没有兑现当前 50 MiB 容量策略。历史 portable projection 会省略历史 ToolResult 图片，所以复现必须保留 live native spans，不能只检查 seed items 的序列化大小。

最小修复边界：统一 User 与 ToolResult 的图片枚举和计量，把计数、逐出与最后的容量判定落在同一载体定义上；容量仍不满足时返回明确结果。保持 tool call/result 身份和原始 Timeline 证据，投影决策的因果记录另按 D1 的约束处理。

验收应同时覆盖 User-only、ToolResult-only、混合图片、live native 与 portable 请求；断言实际 wire、压力估算和事件，而不只测试 User 图片辅助函数。

**F2 · [P2] Memory watcher 的双重状态存在丢唤醒窗口**

[`watcher.rs:102`](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/watcher.rs:102) 先 swap 清空集合，再将独立 `dirty` 标志设为 false。生产线程则在 [`watcher.rs:58`](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/watcher.rs:58) 向集合插入后设为 true。以下交错合法：

1. search 取走旧集合。
2. watcher 向新集合放入 `new.md`，设置 `dirty=true`。
3. search 继续执行 `dirty=false`。

最后集合仍有待处理文件，但 [`backend.rs:230`](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/backend.rs:230) 的 `watcher.is_dirty()` 判定为 false；没有后续文件事件时，之后的搜索不会处理这次修改或删除。

独立双线程程序复用了这几条 ArcSwap/AtomicBool 操作，用 channel 固定上述交错，得到：

```text
taken={"old.md"}, pending={"new.md"}, is_dirty=false
```

这验证的是并发状态机，不是操作系统文件通知的时序测试。将内存序改成 SeqCst 也无法消除上述合法交错。

最小修复边界：优先去掉重复的 bool，从同一个集合快照判断是否有工作；若保留快路径，也必须证明它不会覆盖并发生产者的通知。增加确定性交错回归，而不是增加 sleep 后观察文件的测试。

**F3 · [P2] Memory 向量缓存没有绑定 embedding 模型身份**

[`index.rs:155`](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/index.rs:155) 只检查 `embedding_dimensions`，维度相同直接复用 `chunks_vec`。[`backend.rs:215`](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/backend.rs:215) 打开索引时也只传维度；[`index.rs:512`](/Users/lordcasser/workspace/projects/grow/crates/codegen/memory/src/index.rs:512) 仅按 chunk 是否已经有向量判断缺失。

用户将 embedding model A 改为相同维度的 model B，或修改为另一个使用同名模型的服务端点后，新的 query 向量会与旧文档向量一起参与相似度计算。维度匹配不代表向量空间相同，结果会无明显报错地失真；仅补齐 missing chunks 不能清除旧缓存。这一结论来自完整的缓存身份与查询调用链，未用真实服务量化召回率。

最小修复边界：在现有 index metadata 中绑定不含凭据的 embedding identity，至少包含端点、模型、维度及影响向量的配置。身份变化后失效并重建向量，保留可继续使用的原始 Markdown 和 FTS。两个 session 使用不同配置共享同一 workspace 时，也不能互相覆盖或混用向量身份。

验收：相同维度但不同模型/端点必须失效；身份未变应继续命中；多 session 不能把同一索引写入两个向量空间。

**F4 · [P2] 本机协调的恢复权威仍然依赖 updates.jsonl**

[`coordination/runtime.rs:668`](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/coordination/runtime.rs:668) 的 `durable_inquiry` 从 Grow replay notification 查找 `UiNotice.details`；[`actor/coordination.rs:419`](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/coordination.rs:419) 也从相同来源恢复未结束询问。

这两条路径调用的 [`storage/mod.rs:3619`](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/storage/mod.rs:3619) / `stream_replay_grow_notifications_in` 实际打开 UI updates reader，并跳过无法解析的 replay 行。它们没有从 Timeline 派生询问终态。

因此，当内存结果过期或进程重启后，UI replay 缺失会使原来完成的 inquiry 查询变成 `not_found`；接收侧也无法仅凭 Timeline 找出需要闭合的旧询问。把 audit 放进已有 UiNotice 只减少了文件种类，并没有消除第二份事实权威。

`local-coordination.md` 描述了当前实现，但它与核心文档“UI replay 可以丢失，不能参与恢复”的不变量冲突。这属于明确的架构债务，同时有可观察的结果遗忘路径。本轮做了调用链核查，没有删除或改动用户会话来复现。

独立修复边界：询问 identity、admission/approval、结果及恢复终态进入现有 Timeline，UiNotice 从该事实投影。不要建立新的 inquiry sidecar。验收可在临时会话完成询问后移除 UI replay，验证查询与恢复仍然成立。

**F5 · [P2] Timeline 的正常追加仍携带随全历史增长的工作**

[`mutations.rs:71`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/actor/mutations.rs:71) 为准备一条普通消息克隆整个 Timeline，再在副本上 append。[`Timeline:1535`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/timeline.rs:1535) 派生 Clone，字段包括完整事件 Vec、Surface 和 lifecycle；这不是常数时间的共享快照。消息正文中已有 Arc，但事件容器、String、JSON Value 和生命周期集合仍会被复制。

此外，每条 durable commit 在 [`actor/mod.rs:64`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/actor/mod.rs:64) 提交前后都调用 `next_prompt_index()`；该函数在 [`timeline.rs:2117`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/timeline.rs:2117) 扫描所有事件。纯 observation/request/tool 生命周期也支付这个成本。长会话持续追加时，累计成本可以呈二次增长；压缩只缩 Surface，无法消除全历史部分。

使用当前编译产物的微基准，给 Timeline 放入每条含 1 KiB JSON 的审计事件，重复 20 次消息准备：

| 已有事件 | clone + append 平均耗时 | 直接 prepare 平均耗时 |
| --- | ---: | ---: |
| 1,000 | 0.551 ms | 0.000669 ms |
| 5,000 | 3.604 ms | 0.000744 ms |
| 20,000 | 13.030 ms | 0.000815 ms |

这是本机 dev 产物、无文件 I/O 的准备阶段测试，不是端到端吞吐或 release 性能承诺。直接 prepare 已执行新事件校验；表格用于说明没有必要为准备一条事件复制历史。实际 commit 还要支付自身的验证、prompt 投影和存储成本。

最小修复边界：普通 append 使用现有 `prepare → durable ACK → accept` 路径；把 prompt cursor 作为 Timeline 自己的增量派生投影维护，并处理 rewind。不要增加另一份持久状态、历史快照或并行写 actor。克隆移除与 prompt 投影增量化可以分两个改动验证。

**D1 · 因果记录仍未覆盖所有影响请求的状态与重试决定（清理前）**

这不是说当前模型请求一定失败，而是现有实现达不到核心文档的因果可重建契约，需要分包处理：

- 当前未提交实现的 [`state.rs:189`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/actor/state.rs:189) 持有非持久化 ContinuationLane；[`conversation.rs:1261`](/Users/lordcasser/workspace/projects/grow/crates/codegen/sampling-types/src/conversation.rs:1261) 明确让 NativeContinuationFragment 不可序列化。它会改变下一次 provider 输入，但接受响应时只把 portable items 写进 Timeline。重启后拒绝复活旧 native state可以是正确执行策略；历史审计仍需要保留当时实际使用的证据，两者应分开设计。
- Sampler 的 [`request_task.rs:487`](/Users/lordcasser/workspace/projects/grow/crates/codegen/sampler/src/actor/request_task.rs:487) 先把 retry 放进无 ACK 的事件 channel，然后退避并继续；Shell 的 [`event_tracker.rs:461`](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/event_tracker.rs:461) 继续使用无调用方 ACK 的追加接口。持久 retry 的主要内容是次数与截断原因，无法保留所有被丢弃 attempt 的原始响应/部分流依据。usage settlement 的等待不能代替 retry decision 的 durability barrier。
- 图片容量逐出目前只产生 transient ImageBudget 诊断，缺少精确 source-bound 的历史投影决定。

独立验收应要求：只读 Timeline 与其不可变 artifact 能解释每次实际请求采用什么投影、为什么重试及丢弃了什么；阻塞相应持久化 ACK 时，依赖该决定的下一次请求不能启动。复用既有 Timeline 与 artifact 存储，不增加平行审计日志，也不要求把审计保存的 provider state重新注入运行时。

**D2 · JSONL writer 的首次 prefix 校验仍全量分配（清理前）**

[`jsonl/mod.rs:1245`](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/storage/jsonl/mod.rs:1245) 的 `load_timeline_prefix` 根据完整文件长度分配 Vec，`read_to_end` 后又反序列化出全部事件并重放为 Timeline。它在 fresh writer 缺少 prefix cache，或文件 stamp 改变时执行；正常缓存命中的每次追加不做全量扫描，这一点不能误报。

长会话恢复的首次写入可能同时保有运行中 Timeline、整份 JSONL 字节和另一个解析/fold 结果，形成较高峰值内存。普通 loader 的逐行限长并没有覆盖这个入口。独立优化可复用 bounded JSONL reader，增量 hash 和验证，并消除一次性原始字节副本；仍须保留 seq、schema、内容校验和失败时不写入的语义。

**D3 · 当前架构文档混合了有效约束、旧实现和历史修复记录（清理前）**

两个具体漂移：

- `agent-core-timeline.md` 开头写 schema v24，同文其他段落已经出现 v25；代码常量是 [`timeline.rs:16`](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/timeline.rs:16) 的 25。
- [`behavior-state-overview.md:29`](/Users/lordcasser/workspace/projects/grow/docs/architecture/behavior-state-overview.md:29) 要求顺序应用所有用户 model/effort/Agent 控制，仅允许合并相邻 catalog reload。当前 [`model_switch.rs:957`](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/model_switch.rs:957) 明确 supersede 较早 Sampling desired state，并有对应测试。

不能仅因文档写着队列就认定 latest-wins 实现是 bug。应单独收敛当前控制语义，更新唯一架构说明，将过时描述标为历史；版本号和核心控制矩阵可以与现有测试常量建立轻量校验，无需增加文档生成框架。

**D4 · 仓库内 PR CI 缺少共同的核心回归入口（清理前）**

当前 `.github/workflows` 的自动 PR 测试主要是路径触发的 local coordination 和一个 Windows session creation 测试；release 是手动发布构建。仅改 sampler、Memory、Workflow 或部分通用会话逻辑时，不一定触发验证这些模块的测试。仓库拥有大量有效测试，但这些测试没有完整进入共同的 PR gate。

这是仓库内配置的观察，未检查远端 branch protection 或外部 CI。建议独立补一个覆盖核心 crate 的公共 PR 检查，先运行现有测试与基本构建，再保留专项跨平台测试；不要在修图片或 watcher 的 PR 中混入整套 CI 重构。

**验证记录与限制**

执行过：

```sh
CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test -p chat-state --lib --offline --quiet -- --test-threads=4
CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216 cargo test -p memory -p workflow -p sampling-types -p sampler --lib --offline --quiet -- --test-threads=4
```

| crate | 通过 | 失败 |
| --- | ---: | ---: |
| chat-state | 455 | 0 |
| memory | 292 | 0 |
| sampler | 214 | 0 |
| sampling-types | 264 | 0 |
| workflow | 65 | 0 |
| 合计 | 1,290 | 0 |

另外运行了图片请求/Timeline 准备阶段探针，以及 watcher 确定性交错探针。源码、输出、测试日志和基线记录保存在[审查证据目录](/Users/lordcasser/.codex/visualizations/2026/09/05/01a06f75-b644-7322-b7ef-ed8379e6234a/grow-review)。探针链接本次工作区编译产物，没有修改生产源码或仓库测试。

现有测试通过不等于上述边界已覆盖。本轮没有运行完整 Shell/Pager 测试、实机跨平台恢复、真实模型端点或完整端到端 UI 回归。

原审查的处理顺序建议（现已按边界落地）：先独立修 F1 和 F2，再处理 F3/F4；F5 按长会话增量成本单独优化。D1 涉及记录语义，先明确契约并分开处理 native evidence、retry ACK 和请求投影；D2、D3、D4 分别作为存储、文档、CI 工作项，不并入功能修复。
