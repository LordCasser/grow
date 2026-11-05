# Client / UI / terminal / render 分区审计

## 结论与边界

审计对象为 `pager`、`pager-minimal`、`pager-render`、`markdown`、`mermaid`、`ratatui-inline`、`ratatui-textarea`、`client-support`、`acp-transport`、`shell-base`。源码位置以仓库 HEAD `d6128138c` 与可见工作树为核对基线；46 个发布标签按各自 Git tree 单独计算。工作树其他分区有未提交工作，本报告没有修改实现或覆盖他人文件。

当前结构有清楚的主要所有者：`pager` 持有会话 UI 与 reducer；`pager-render` 持有终端和共享 presentation/host 能力；`pager-minimal` 只持有 Minimal 的绘制策略；Markdown、Mermaid、textarea、inline terminal 和 ACP 分别持有局部转换或协议状态；`client-support` 持有跨 UI 的 OS 能力；`shell-base` 提供 shell 家族的基础功能。Minimal 的函数指针 hook、分帧 transcript build、异步文件队列、Mermaid 子进程和 terminal writer barrier 都有明确生命周期边界。

未证实会话串线、异步结果误投或终端资源泄漏的当前缺陷。审计确认两个可定位的独立风险/债务：ACP 通道和 handler 调度没有容量上限；Markdown 未闭合代码围栏的增量高亮避免了重复解析，但仍按每帧克隆已格式化输出。两者均为静态结论，没有本轮运行负载测试证明具体失效阈值。其他抽象边界与已固化的恢复规则见下文。

本轮按父代理协调要求只做静态审查与轻量 Git 历史核对；没有运行 Cargo、PTY、构建、OpenSpec validate 或 archive。代码中存在相关测试，不等于本轮测试通过。历史覆盖包括所有本地 tag，但不是逐提交逐行审查，也不代表未取回的远端历史。

## 职责、状态与依赖方向

| 模块 | 主责与状态 owner | 主要依赖/调用方向 | 观察 |
|---|---|---|---|
| `pager` | 全屏 TUI composition root、AgentView/scrollback、输入分发、ACP 结果 reducer、任务与 transcript effects、挂起恢复。 | 依赖 `pager-render` 并重导出其 appearance/render/terminal 等 API；直接使用 `markdown`、`mermaid`、`ratatui-textarea`、`ratatui-inline`、`acp-transport`。 | Agent/session 和 UI 状态集中在 `AppView`/`AgentView`；模块树按 `root`、`agent_view`、`session`、`scrollback` 分开，但跨域状态迁移最终仍由 pager reducer 负责。见 `pager/src/lib.rs:30`、`:68`。 |
| `pager-minimal` | Minimal 分帧构建 transcript、stamp live-tail 展示模式、决定 committable frontier，并把稳定块提交到 terminal native scrollback。 | 只依赖 `pager` 读取受控 view-model facade；终端写入经 `PagerTerminal`/`ratatui-inline` 能力；由 CLI composition root 安装 pager hook。 | 是有意的反向依赖，但调用面窄：`pager` 不依赖该 crate，hook 由组合根接线，`minimal_api` 只开放必要状态操作。`pager-minimal/Cargo.toml:7`、`pager/src/lib.rs:30`。不要把它改造成第二个 Agent/session owner。 |
| `pager-render` | 公用 TUI primitives、主题/外观、终端探测和能力、绘制、剪贴板/链接/图片以及低层 render helpers。 | 依赖 `markdown`、`ratatui-textarea`、`ratatui-inline`、`client-support`，同时依赖 `config`、`tools`、`paths`、`diagnostics`。pager 从根部重导出以保留消费路径。 | 它是 presentation 加 host adapter 的共享层，不是纯绘图 crate。终端能力探测/OS 粘贴板必须有 host 依赖；若把它拆成纯绘图层，需先给这些能力定义注入接口，避免为了分层复制策略。清单见 `pager-render/Cargo.toml:20`、`:60`、`:68`。 |
| `markdown` | Markdown/LaTeX 解析、流式冻结与 tail 重绘、语法高亮、hyperlink/source-map 输出。 | 依赖 ratatui、pulldown-cmark、syntect 等通用库；不依赖 pager/session 状态。`pager-render` 与 `pager` 消费其结果。 | `StreamingMarkdownRenderer` 自己拥有 source、checkpoint、缓存和 link id；主题/宽度变化负责失效缓存。输入块由上游消息 owner 持有，不在 parser 内重复会话状态。`markdown/src/lib.rs:30`、`markdown/src/streaming.rs:220`。 |
| `mermaid` | Mermaid source 到 SVG/PNG 的 engine facade、输入限制、栅格化和子进程 timeout/reap building blocks。 | pager 将其作为模型/工具输出消费契约；第三方 `mermaid-to-svg` 实现归 build/vendor 分区，本报告不审其内部。 | `mermaid/src/lib.rs:20` 明确区分 `catch_unwind` 和 panic=abort；真正隔离由 pager 短生命周期子进程提供。 |
| `ratatui-inline` | inline viewport、终端历史输出、尺寸变更、ANSI 行切分及 semantic link 输出。 | 下层依赖 ratatui/crossterm；`pager` 通过 PagerTerminal 使用，`pager-minimal` 在 commit/native-scrollback 路径使用。 | 和普通屏幕 redraw 不同，它拥有 native scrollback 写入语义；commit frontier 的高度计算必须和 live tail 对齐。`pager-minimal/src/commit.rs:30`。 |
| `ratatui-textarea` | 可复用文本编辑控件、EditPlan、selection、undo/redo、鼠标和 clipboard provider 接口。 | 不依赖 pager；pager-render 重导出剪贴板接口并提供 SystemClipboard adapter；pager 持有 prompt/editor 的业务上下文。 | 控件保存编辑/undo 状态；它不决定 Agent、session 或粘贴结果的业务落点。外部异步粘贴必须由 pager 绑定目标身份并验证完成结果。`ratatui-textarea/src/textarea.rs:195`。 |
| `client-support` | OS clipboard、附件读取与图片数据、session 路径等 shell/client 共用能力。 | 位于 `shell` 上游；依赖 `tools`、`config-types`、`tty-utils` 和平台 clipboard backend。`shell-base`、`pager-render` 直接消费。 | OS 过程和临时文件归此层；TUI toast、焦点目标、粘贴是否仍适用归 pager。macOS image 路径使用私有目录、受限 `osascript` 进程组和有界读取。`client-support/src/lib.rs:1`、`client-support/src/clipboard.rs:1040`。 |
| `acp-transport` | ACP typed message/channel、SDK v1 wire facade、handler dispatch 与 `!Send` 连接适配。 | pager/headless、shell 权限/Prompter 等消费者依赖它；它不拥有会话 UI 或策略状态。 | `AcpChannelFailure` 区分 enqueue 失败与 response channel 关闭，错误仍用 JSON-RPC `INTERNAL_ERROR` 并以 data tag 供调用方判别。`acp-transport/src/common.rs:20`。 |
| `shell-base` | shell crate 家族的基础 env、CPU profile、process/filesystem helper facade。 | 依赖 `client-support`、`config`、`tty-utils` 等下层能力；不反向依赖 pager。 | 不是 UI owner。其 clipboard/session helper 重导出需要维持 shell 上游边界，不能把 pager 通知策略移入此层。`shell-base/src/lib.rs:1`、`shell-base/Cargo.toml:20`。 |

### 关键执行链

- **Terminal 输入与输出**：`ReaderThread::spawn` 独占 crossterm poll/read，给事件加到达时间并发送给 event loop；event loop 将其归约为 Action/状态变更，再由 `Effect`/`JoinSet` 执行异步 I/O，完成后经 `TaskResult` 归约。输入暂停、reader park 和 teardown join 属于 event loop/ReaderThread。绘制端由 `pager-render::TermWriter` 为每帧预留序号，发入有界 `std::sync::mpsc::channel`；后台 writer 递增已写序号，child suspend 先 park 输入再 wait_drained，避免子进程接管终端后旧帧落下。证据：`pager/src/app/reader_thread.rs:31`、`pager/src/app/root/event_loop.rs:1260`、`pager-render/src/render/draw.rs:70`、`:350`。
- **Minimal**：pager 调用注册的 hook；Minimal 每帧限时 pump transcript build，按 owner/session/build generation 快照 ID，完成后排队后台文件写。完成 reducer 重新核对 generation、root/child agent id 和 session id，再创建 `PendingPager`。tag/reload 期间清空 staging 并重启 build，避免输出部分 replay 前缀。证据：`pager-minimal/src/full_view.rs:27`、`pager/src/minimal/api.rs:435`、`:597`、`:747`、`:756`。
- **外部分页器**：`PendingPager` 拥有临时文件及 root/agent/session 来源；挂起超时将同一 request 放回队列，恢复后重新尝试；成功/失败在 restore 后反馈，身份不匹配则不向新会话写旧消息。事件循环的 suspend/restore 复用 reader park 与 writer drain。证据：`pager/src/app/external_pager.rs:7`、`pager/src/app/root/event_loop.rs:767`；契约见 `client-surfaces` 的 “External pager transcripts have private owned temporary files” 和 “External pager feedback preserves request origin”。
- **Markdown/Mermaid**：Markdown stream owner 是每个消息渲染块；theme/style/width 变化清除对应 frozen/highlight cache。Mermaid detection 只抽 source/ranges、不在流式消息创建时渲染；AgentView 首次点击才启动单 worker，按 source/theme/quality/宽度 bucket 生成 cache key，子进程 timeout 3 秒、失败降级为 code block。结果以 key 匹配 pending action；session replay reset runtime 后，旧结果不能触发新会话 open/copy/toast。证据：`markdown/src/streaming.rs:220`、`pager/src/scrollback/blocks/mermaid_content.rs:153`、`pager/src/app/agent_view/mermaid_worker.rs:228`、`:876`、`:985`、`:1173`。
- **ACP**：SDK 消息进入 typed `acp-transport` channel/gateway，再由 SDK connection adapter 转到 Agent/Client handler；leader 的 raw JSON bridge 在专用线程上用 current-thread runtime + `LocalSet` 适配 SDK 内部 `spawn_local`，避免把 `!Send` handler 跨线程送递。prompt cancel 走 adapter 对应取消路径。channel 错误有可判别 tag；但请求容量/并发限制见发现 ACP-1。证据：`acp-transport/src/channel.rs:27`、`gateway.rs:170`、`:249`、`pager/src/acp/leader_bridge.rs:100`、`connection.rs:37`。
- **Clipboard 与异步结果**：OS 读写由 `client-support`，pager-render 提供 `SystemClipboard`/OSC52 等策略入口；textarea 仅持 provider 和 copy 通知槽位。Pager 对 clipboard attachment、image viewer、文件任务结果按原始 target/view/session identity 做 reducer 校验。显式 transcript copy/export 串行、限长队列，失败和 panic 转成结果并推进后续任务。macOS clipboard script 对 stdout/stderr 各限 1 MiB、设置 deadline、结束时 kill 进程组，管道尾部最多等 300ms；Image transfer 另有字节预算。证据：`pager-render/src/clipboard/mod.rs:229`、`client-support/src/clipboard.rs:1040`、`pager/src/app/transcript_file_writes.rs:1`；契约见 `client-surfaces` 的 clipboard、image、export 和 “Transcript file jobs preserve order and origin” 场景。

## 发现（按影响排序）

### ACP-1 — 通道容量和 handler 并发无上限（独立资源债务，未复现）

- **触发条件**：ACP peer/producer 在 gateway 或消费者处理变慢时持续推送请求；单次高并发也会让 handler future 同时等待较慢的 session、terminal 或 permission 操作。
- **位置与机制**：`acp-transport/src/channel.rs:10` 将双向 channel 暴露为 `mpsc::Unbounded*`，`:27` 由 `acp_channels()` 创建 unbounded channels，`:34` `acp_send` 立即 enqueue 后等待 oneshot；`acp-transport/src/gateway.rs:170` 和 `:249` 对每条消息调用 spawn callback，默认 `spawn_local`，没有 in-flight semaphore/上限。raw bridge 的 8 MiB simplex 是 SDK 编解码字节管道容量，不限制 gateway message queue 或 handler task 数（`pager/src/acp/leader_bridge.rs:20`、`:110`）。
- **受损不变量与后果**：消费速度慢时，待处理 message/oneshot/handler future 数随生产者输入增长；本分区没有提供“队列有界或 producer 背压”的保证，可能将协议流量转成无界内存与 task 增长。现有证据不能证明本地正常 CLI 会达到失效阈值，所以分类为容量设计风险，而非已观测故障。
- **Owner**：`acp-transport` channel/gateway；SDK/CLI composition root 负责选择合适的并发预算和过载语义。
- **可证伪通过标准**：给入站/出站消息和并发 handler 明确预算；慢 handler 加速率受限 producer 压测时，队列与活跃任务不超过预算，饱和时按明确协议错误/背压规则结算请求，恢复后后续请求可成功且不存在丢失 oneshot。若协议/调用方已有可证明的外部上限，也可用该契约和相应负载上限证伪本风险；本次静态审查未找到此类限制。

### MD-1 — 开放代码围栏的命中输出仍逐帧克隆（已知性能债务）

- **触发条件**：长 fenced block 持续逐小块到达、且每次输入都会 redraw；特别是块一直位于无法 checkpoint 的 Markdown tail。
- **位置与机制**：`markdown/src/open_fence_highlighter.rs:138` 的 open-block cache hit 返回 `hit.clone()`；`highlight` 在 `:259` 有明确 TODO，说明开放块返回会按行复制预格式化 spans。高亮解析本身已增量化，但当前帧仍需复制已完成行。闭合块 memo 有 256 KiB source-byte cap（`:44`），该 cap 不解决 open-block 命中复制成本。
- **受损不变量与后果**：每帧的输出内容正确，syntect 解析避免了原有重复 O(N²) CPU；但复制量仍与当前块长度成正比，逐行流式输入时整个生命周期累计可达到 O(N²) 字节复制，长回复仍可能占用 render 热路径。代码注释将此作为接受的 residual，不是输出错误。
- **Owner**：`markdown::OpenFenceHighlighter` 与 `StreamingMarkdownRenderer` 的输出借用/快照边界。
- **可证伪通过标准**：对逐行增长的长 fence 测量高亮解析与 clone/输出分配分别计数；若 clone 成本在代表性长度内不产生可测帧预算退化，可将影响降级；若要关闭债务，缓存行与输出层改为可复用/借用表示后，需与 batch 输出逐字节一致，并证明累计复制不随已提交前缀每帧重复增长。

### 未列为缺陷的结构选择

`pager-minimal -> pager` 的依赖看起来与单向分层相反，但这是当前有意的组合方式：`pager` 通过 `minimal_hook` fn-pointer 接入外部实现，`minimal_api` 集中暴露读取/写入门面，CLI 安装 hook；pager 不直接依赖 `pager-minimal`，没有 crate cycle。若未来不再需要独立 Minimal release/build 边界，才有理由比较是否改为 pager 内部模块。当前没有证据支持为了形式上的单向图而扩大 trait/bridge 框架。

`pager-render` 是跨 UI 的共享层，包含 host/clipboard/terminal probe，不只是 ratatui view primitive。其 Cargo 依赖能清楚看到该取舍；新增消费者若只需要纯渲染会间接携带 host 支持依赖，这是独立的分层权衡，不足以单独证明当前运行问题。重复维护 OS clipboard backend 或 terminal capability cache 才会构成需要重审的具体旁路。

## 已有防护、契约与验证证据

| 主题 | 当前源码不变量/现有回归证据 | 本轮验证状态 |
|---|---|---|
| 最小 transcript owner/reload | build 持有 root/agent/child session identity 与 generation；完成时重新解析 owner；reload 清 staging 并重启。 | 静态追踪 `minimal/api.rs`；未跑 Minimal/PTy 测试。 |
| 外部 pager | pending request 拥有 private temp path、ANSI 与来源；timeout requeue 保留同一请求；错误在终端恢复后反馈。 | 静态追踪 event loop 和 `PendingPager`；未执行 child suspend。 |
| Mermaid key/action identity | cache key 含 source hash/theme/quality/width bucket；pending action 按 key 一次收敛；reset 丢弃旧 receiver。源码含 `take_pending_for_takes_all_matching_and_leaves_the_rest`、`replay_detaches_pending_actions_and_late_results`。 | 阅读代码与现有测试定义，未运行。 |
| ACP 错误分类 | SendFailed 与 RecvFailed 用 typed data tag 区分，避免对 message 文本做 substring 判断；gateway handler 由 LocalSet/注入 spawner 调度。 | 阅读 channel/gateway/bridge；未运行模拟断连或并发。 |
| Terminal suspend/output | 单一 reader 所有权；writer 序号 barrier 防止 child handoff 后迟到帧；恢复后全屏重绘。 | 阅读 `ReaderThread`、`WriterSync`、event loop；未运行 TTY 实验。 |
| Clipboard process/byte limits | macOS `osascript` stdout/stderr、deadline、process group、drain grace 和 image encoded-byte budget 有独立保护；OpenSpec 同步描述限制边界。 | 阅读实现与测试代码；未运行 macOS 真实 pasteboard/platform 测试。 |
| Markdown streaming | style/table width/soft break 改变会清 checkpoint/highlight state；open fence append-only 检查失败则重建；closed memo 有 source-byte cap。 | 阅读实现和相关 differential/regression 测试；未执行 benchmark/fuzz/Cargo。 |

当前规范 `openspec/specs/client-surfaces/spec.md` 包含 UI 分区相关的剪贴板统计与交付证据、transcript atomic write/owner/order/queue cap、外部分页器文件/来源/suspend retry、clipboard image metadata race/temp/process/byte budget、drop/image/viewer owner 等场景。当前源码入口与这些要求相吻合的部分已逐项核对；本报告没有发现可据此提出的契约 delta，也未修改规范。

## 46 个本地 release tags 覆盖

### 计算口径

- 发布标签由 `git tag --list 'v[0-9]*' --sort=version:refname` 完整枚举：46 个，从 v1.0.0 到 v2.6.0。
- `Commit` 列为 `git rev-parse <tag>^{commit}` 的前 12 位；`tree` 是下列十个逻辑 crate 的递归 Git tree listing 原始字节 SHA-256 前 12 位：`pager`、`pager-minimal`、`pager-render`、`markdown`、`mermaid`、`ratatui-inline`、`ratatui-textarea`、`client-support`、`acp-transport`、`shell-base`。v1.0.0 使用当时目录名：`grow-pager`、`grow-pager-minimal`、`grow-pager-render`、`grow-markdown`、`grow-mermaid`、`xai-ratatui-inline`、`xai-ratatui-textarea`、`grow-shared`、`xai-acp-lib`、`grow-shell-base`；此外 v1.0.0 仅存在的 `grow-markdown-core` 也纳入 v1.0.0 源码树和 manifest 指纹，归到 markdown 历史职责。不能把旧名路径查空当作空树。
- `manifest` 是根 `Cargo.toml` 和上述逻辑 crate manifest 的递归 `git ls-tree` listing SHA-256 前 12 位；v1.0.0 另含 `grow-markdown-core/Cargo.toml`。`Δ` 是与前一相邻 release tag 的 `git diff --find-renames --name-status` 中，审计 crate 变化记录数；重命名归到目标路径，每条 rename 算一项，模块后括号为模块计数。它表示路径记录数，不是语义改动行数。
- 完全相同 tree hash 可复用代码树检查；`Δ=同` 表示审计范围相邻 tree 无差异。若 tree 相同但 manifest hash 不同，经 manifest diff 核对只是根 workspace `Cargo.toml` 变化，并非 crate 源码变化。

| Tag | Commit | tree SHA-256/12 | manifest SHA-256/12 | 相邻 tag 审计 crate 路径记录数 Δ |
|---|---|---|---|---|
| v1.0.0 | `da034e22d87b` | `ffc26b8edaa3` | `68b3b17fec91` | 基线（10 crate 使用旧目录名；另含 grow-markdown-core） |
| v1.1.0 | `7ea59a9db583` | `c32fb8f631e5` | `223ac2399888` | 979：pager 778、minimal 15、render 71、markdown 46（44 次迁移/修改记录 + 2 个旧 markdown-core 文件删除）、mermaid 10、inline 13、textarea 16、client-support 10、ACP 9、shell-base 11 |
| v1.1.1 | `4f8e0232eae0` | `ac8550a7f9bc` | `b886d81abe39` | 1：pager 1 |
| v1.1.2 | `abca23e0b295` | `cfaca2d99432` | `33111ffd4faa` | 38：pager 38 |
| v1.1.3 | `1b24b9f03668` | `5f2f374d6ec5` | `e472c180ae29` | 3：pager 3 |
| v1.1.4 | `28b798dc17b2` | `43ee215b9e45` | `3e7651ae9644` | 56：pager 47、render 1、markdown 1、inline 3、textarea 2、client-support 1、shell-base 1 |
| v1.1.5 | `05a3fbcc57c1` | `220773338220` | `aed4938e583c` | 171：pager 160、minimal 6、render 5 |
| v1.1.6 | `ca3400250af9` | `71789c3d0df1` | `b288cc01c271` | 45：pager 45 |
| v1.1.7 | `62d9b47fd9d5` | `f0775a2d3d3f` | `986ee4774d9c` | 27：pager 27 |
| v1.1.8 | `bde0050632c2` | `600bbc98b3a2` | `33aa3f40880e` | 37：pager 37 |
| v1.1.9 | `465d9b12a42d` | `82bf8ac63ff0` | `0d65bd776409` | 17：pager 17 |
| v1.1.10 | `38b2d7fc7039` | `339a357da3f7` | `7b22baad1eb9` | 24：pager 23、client-support 1 |
| v1.1.11 | `79495bcd40e5` | `339a357da3f7` | `005b305b5cfb` | 同 tree；根 manifest 变化 |
| v2.0.0 | `2282940c1120` | `a3db17c65388` | `a549b82d2a73` | 351：pager 326、minimal 11、render 7、inline 2、client-support 3、shell-base 2 |
| v2.0.1 | `ae8df2a3b4b9` | `69c9e4eca84e` | `9298ba8a011e` | 62：pager 61、client-support 1 |
| v2.0.2 | `7233f4e51e6d` | `2effa9c7fb9f` | `ebcf74799d28` | 133：pager 117、minimal 6、render 1、markdown 7、inline 2 |
| v2.0.3 | `5aabf367b53a` | `45361e40f9f3` | `66ec488359e5` | 8：pager 8 |
| v2.0.4 | `1b52597ccdb5` | `d442d880f700` | `45f0f4ed9101` | 62：pager 59、render 1、client-support 1、shell-base 1 |
| v2.1.0 | `d43756888adf` | `2107d070c6e0` | `6ee383d28e40` | 121：pager 103、minimal 2、render 2、mermaid 1、client-support 2、ACP 11 |
| v2.1.1 | `96565d6975d8` | `19fe165ccc6b` | `f7b79f514317` | 8：pager 8 |
| v2.1.2 | `1ca5c6b120ea` | `d8ab972bc2eb` | `a85f3f9a52aa` | 55：pager 47、minimal 4、render 1、markdown 3 |
| v2.1.3 | `09c68b808c41` | `e6ef19e6d735` | `62f2049c02bf` | 18：pager 18 |
| v2.1.4 | `87d1df4c3b0a` | `b0031b2bea89` | `dc7d102e2e71` | 5：pager 5 |
| v2.1.5 | `a8dc93716f9a` | `67f74c6417e3` | `d3f65d678f5e` | 122：pager 101、minimal 5、render 13、client-support 3 |
| v2.1.6 | `2a8297e818b6` | `b99711224b63` | `ce8f10585196` | 22：pager 22 |
| v2.1.7 | `9f721d8e927c` | `9b13dfedeb16` | `bc854743be65` | 34：pager 31、minimal 1、render 2 |
| v2.1.8 | `7ed6441a8fb8` | `0132ccdab98b` | `57bf20f7bfd7` | 2：pager 2 |
| v2.1.9 | `bd1f5085a9c9` | `a9df5a2bdb91` | `3beae6a6d0de` | 7：pager 7 |
| v2.1.10 | `c17b085d07f8` | `a9df5a2bdb91` | `40550b1e4b2b` | 同 tree；根 manifest 变化 |
| v2.1.11 | `3024dad1ef75` | `a9df5a2bdb91` | `038966852320` | 同 tree；根 manifest 变化 |
| v2.2.0 | `a84ce03b0d13` | `9d44c752594e` | `4fee2a2fb413` | 167：pager 140、minimal 8、render 10、markdown 2、mermaid 1、inline 3、client-support 3 |
| v2.2.1 | `b38627016b41` | `283582b90b62` | `33ac30a523fa` | 27：pager 24、render 1、client-support 1、shell-base 1 |
| v2.2.2 | `afbe3e48209d` | `9fa7025ae000` | `55d99a984bf4` | 19：pager 19 |
| v2.3.0 | `ef665ec41eac` | `9cf0cfa91abf` | `94e1fa68d84a` | 50：pager 47、minimal 3 |
| v2.3.1 | `7b7d56ab98a6` | `34e0dd789e3c` | `fda3d4f654cd` | 11：pager 10、minimal 1 |
| v2.4.0 | `cd5d88ec7f5d` | `5996ece25854` | `a974cc50e105` | 104：pager 103、minimal 1 |
| v2.4.1 | `5fcbd12b0c10` | `700807ad1fc2` | `e51fad3edd2e` | 44：pager 41、minimal 2、client-support 1 |
| v2.4.2 | `ecb0f9cc6b48` | `2c1b73113cdd` | `0a2c9a7f880a` | 1：pager 1 |
| v2.4.3 | `2069c43db5a3` | `ff6b9c928fa8` | `2f89d3891040` | 18：pager 18 |
| v2.5.0 | `8443a4d1648d` | `ddad357eaa89` | `f9818859ba17` | 115：pager 103、minimal 1、render 5、markdown 2、client-support 2、ACP 1、shell-base 1 |
| v2.5.1 | `a58b5dd7e8c7` | `666a698216d7` | `8f8624701efa` | 20：pager 20 |
| v2.5.2 | `1c88fccb4adf` | `99df94f85723` | `877e3216b13e` | 36：pager 35、render 1 |
| v2.5.3 | `fce2f3d590dd` | `99df94f85723` | `9ec23daa6276` | 同 tree；根 manifest 变化 |
| v2.5.4 | `069f712894ca` | `d55ca47c6655` | `a19aed7e6c2f` | 1：pager 1 |
| v2.5.5 | `599873595c94` | `482a1052dd00` | `3932ca219e6e` | 33：pager 31、minimal 2 |
| v2.6.0 | `5475dff997f0` | `0c6c634e9ff2` | `31b3f1fc82a4` | 90：pager 86、minimal 2、render 2 |

### 有架构意义的历史节点

- **v1.0.0 → v1.1.0：crate 改名与职责拆分**。`git diff --find-renames` 确认旧名到新名的高相似度迁移：`grow-pager → pager`、`grow-markdown → markdown`、`grow-mermaid → mermaid`、`grow-shared → client-support`、`xai-acp-lib → acp-transport`、`grow-pager-render → pager-render`、`grow-pager-minimal → pager-minimal`、`xai-ratatui-* → ratatui-*`、`grow-shell-base → shell-base`。`grow-shared` 的 `clipboard`、`placeholder_images`、`session`、`stderr`、`ui_config` 源文件均以 90–100% 相似度迁到 `client-support`，职责仍是 shell 上游共用的 host/session support。独立的 `grow-markdown-core` 仅在 v1.0.0 存在：它提供轻量 `pulldown-cmark` parser options、offset event/单波浪线策略和 headless analysis API；v1.0.0 唯一 Rust crate 消费者是 `grow-markdown/src/parse.rs` 调用 `offset_events`。v1.1.0 删除该 crate 和 workspace dependency；parser policy 被移入 `markdown/src/parser_policy.rs`，渲染路径继续复用，分析 API 没有在本区保留独立 crate。该两文件删除已纳入 markdown 路径变化计数。v1.0.0 和后续代码都保留 `minimal_hook`/`minimal_api` seam；`pager-render` export 同样可由两个版本的 pager `lib.rs` 直接核对。迁移树记录数高，不能把它解释为独立设计变化数量。
- **v2.0.0：pager app 目录按 owner 分组**。`git diff --find-renames v1.1.11 v2.0.0` 显示 `app/dispatch → app/root/dispatch`、`app_view.rs → app/root/mod.rs`，以及 `agent_view`、`session` 文件迁移；大多数移动相似度 80–99%。同期提交线索包括 `f0cbdb685`（minimal seam）、`fca52658f`（minimal commit state）、`962ac387d`（runtime facts 移入 AgentSession）、`6508f9933`（session ownership）。v2.0.0 `pager-minimal/src/lib.rs` 已将 CLI 作为 hook installer，并把 native scrollback commit 按 frontier、resize、print-once 的顺序组织，说明此版本主要是 owner/目录重整及 Minimal 生命周期巩固。
- **v2.1.0：ACP SDK 适配层替代旧 normalize 路径**。差异新增 `acp-transport/src/connection.rs`、`handler.rs`、`protocol.rs`，删除 `normalize.rs`；历史源码 `connection.rs` 已有双向 `connect_*_v1` adapter 及 LocalSet 测试路径，channel 仍采用 unbounded + oneshot。提交 `4c257777f` 明确为 stable v1 stack 迁移到 Rust SDK 2。当前 connection/channel/gateway 的责任边界延续于此，容量债务不是仅在 HEAD 新增。
- **v2.2.0：terminal 生命周期与输入/回放功能继续拆分**。新增 `pager-render/src/terminal/pop_fence.rs`、`pager/src/app/reader_thread.rs` 和 communication scrollback block 等。v2.2.0 历史源码已可见 reader 单一线程、bounded join grace 与 pop fence 等待 terminal reply 的做法；相关线索为 `d30f10f35`（runtime event delivery/recovery）、`794c65c62`（bounded UI work）。
- **v2.3.0：session replay/display projection 进入 pager**。新增 replay command 与 transcript projection 子树，pager/minimal 两边都有变更。职责仍是 pager 持有 session-view/replay owner，Markdown 只消费展示文本。
- **v2.4.0–v2.5.0：会话退出、resume 与资源发布合同继续收敛**。相邻路径数据显示变化仍集中在 pager，随后 `v2.5.0` 有 pager-render、markdown、client-support 和 ACP 小范围调整；提交线索包括 `8e80b1ccc`（2.4 session exit）、`567560b82`（2.5 timeline/resume）、`355483863`（resource scopes/canonical paths）。这些变化不改变该分区的主 owner 图。
- **v2.6.0：近期变更仍集中在 pager composition/reducer**。相对 v2.5.5 有 86 条 pager 路径记录，minimal 2、render 2；新增的 session deletion dispatch 位于 `app/root/dispatch/session`。当前 Mermaid worker 和 minimal generation/owner 检查可由 v2.6.0 tree 静态核对；本轮没有执行该 tag 的构建或 PTY 验证。

## 可复核范围与未完成验证

历史指纹、manifest 与相邻目录差异均已覆盖全部 46 个本地 release tag；重点迁移以 rename detection 加代表版本源码检查。没有检查每个标签的每一行实现，也没有逐标签重跑旧版工具链。HEAD 审查覆盖了十个 crate 的 manifest、公开入口、主要调用链及相邻依赖，但无运行时生命周期实验。最直接的补充验证是后续统一 Cargo 窗口中运行父代理安排的定向测试；对于 ACP-1，现有测试之外还需有慢 handler/flood 背压实验；对 MD-1，需定向 benchmark。报告不据未运行测试声称功能验证通过。
