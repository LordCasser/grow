## Context

入口、authority、父子关系与代码证据见 [analysis.md](analysis.md)。本 change 先完成可独立交付的只读导出；`../replay-session-transcripts/` 在其基础上加入播放，不把播放器提前实现进来。

当前工作树包含用户已有的 Pager Ask 展示、Workflow discovery 和相关规范/文档修改。本 change 保留这些并行变更，不覆盖其内容。

## Goals / Non-Goals

**Goals**：离线导出完整的可恢复用户对话；按直接委派层级组织独立 transcript；CLI 和 trajectory 共用读取、可见性及 Markdown 规则；给 replay 留下带来源的展示读取入口。

**Non-Goals**：执行或恢复 agent；改变 Timeline 持久格式；保存精确 token 录像；导出内部审计证据包；自动包含 fork 祖先、resume 来源或 Sideband 为 agent；更改 TUI `/export` 的现有产品行为；全量拆分 AppView/AgentView；建立通用 event bus、插件式 exporter 或新的 crate。

## Decisions

### 1. 目录与命令语义

```text
grow export <session-id> [output-dir]

<session-id>/
  transcript.md
  subagents/
    <child-session-id>/
      transcript.md
      subagents/
        <grandchild-session-id>/
          transcript.md
```

默认路径以命令调用时的 cwd 为基准，不使用 session 的 cwd。显式 `output-dir` 就是输出根目录，展开 `~`，相对路径基于调用 cwd，不再额外追加 session ID。目录名用完整 canonical session ID，标题仅作 Markdown 元数据。拒绝非法路径分量、重复身份与相同节点的多父关系，不能通过清洗 ID 将不同 session 合并。

每份文档有 session ID、可选标题、自己的 transcript 和相对链接形式的直接子文档索引。索引不复制 child 正文到 parent。没有可见消息的合法空 session 仍有文档并标明没有已记录正文。删除 CLI clipboard 参数及其专属反馈 helper/测试；TUI 的写文件、剪贴板反馈和紧凑 renderer 保留。

已有输出路径（文件、目录、符号链接，包含空目录）统一拒绝，不引入覆盖、合并或自动编号选项。成功时向用户报告输出目录与 agent 数量；stdout 如有输出，只放结果路径，不再放 transcript 正文。

默认目录已经存在时，错误和帮助应提示使用另一个显式 output-dir，或由用户自行移动/删除旧导出后重试；工具不能自动处理旧目录。用户指南需要说明重复导出的这一行为。

### 2. Shell 负责来源、验证和展示事件读取

在现有 `shell::session::storage` 下增加窄的 transcript reader 模块。其职责是解析 canonical session、固定读取边界、构造委派拓扑、展开已验证的 ACP/Grow 展示事件。返回类型只承载已有事实的派生值，不成为持久权威。Pager 不自行遍历 `~/.grow` 或解析裸 JSONL。

最小读取单位是单个 session 的固定 transcript 来源，树遍历是其上的导出编排。后续 replay 只看一个节点时不应被迫加载所有后代正文，或因无关兄弟材料缺失而失败；需要验证的 parent/seed authority 仍按该节点的实际来源解析。保持这两个职责即可，不建立可扩展查询框架。

内部先读取身份与拓扑，按 pinned directory/file handle 和已提交长度固定每个必要 ledger。遍历从指定节点开始；如果节点是 subagent，通过 seed 的 lifecycle owner 找到拥有相关 spawn 的 ledger，建立候选索引，再按 `security_parent_session_id` 选择从请求根可达的节点。允许 nested spawn 记在 root ledger，也允许记在各自 owner ledger。逐条核对 child summary、seed 的 owner/seq/identity，以及存在的 terminal result ref。fork、`resumed_from`、Sideband、Workflow journal 是来源/运行关系，不自动成为目录父子边。

只读 snapshot 不要求整个跨 session 树拥有同一墙钟时刻，也不对活跃 session 追尾。每个 ledger 的截点固定后按现有 response reconciliation 检查跨引用。源正在变化而缺少必要前件、required child 尚未落盘、身份冲突或损坏时明确失败并提示重试；不能静默跳过 child，也不能启动 writer 修复。有效的失败 spawn 若根本没有 child ledger，同样报告该节点不可导出，不伪造 child transcript。

树读取预算参考现有 trajectory：最多 512 个必要 session/owner 节点、深度 32、总读取源字节 512 MiB、总 Timeline 事件 250,000；单 JSONL 记录沿用 64 MiB 边界。字节统计包括实际消费的 updates 和引用材料，不只检查 metadata。按 session 逐个投影、写入临时输出并释放 typed payload；不一次物化整棵树的全部 RenderBlock。实现中如发现这些额度与合法 fixture 冲突，先记录和调整明确边界，不能移除预算。

保留完整 notification envelope 和 meta，以及可追溯的来源位置、原始时间依据。沿用 response projection planner，复用 rewind 过滤、strip_context_wrappers、admission identity/digest 校验。storage-only projection 不作为用户事件暴露；合成 envelope 的读取时刻不替代源时间。这个 carrier 为后续 replay 服务，不新增磁盘事件字段。

### 3. Pager 负责可见性、会话展示投影和 Markdown

CLI 现有的 ACP-only reader 加 `render_blocks_to_markdown` 不够。增加完整 transcript 的专用入口，保留旧紧凑 renderer 供 TUI 使用；公共的工具内容/消息转换抽成普通 helper，避免两份规则逐渐分叉。

最小复用单元是已有 tracker、ScrollbackState、RenderBlock 和所需只读展示 store。对于目前绑在 AppView 上的 Grow handler，只抽取实际生成/更新展示数据的部分，live handler 与离线入口共用；连接、请求回复、通知服务、自动跳转和任务控制仍在 live orchestration。禁止把整个 ACP handler 作为离线投影器调用，也不复制一个简化的第二套 AppView。

| 输入类别 | 完整 transcript 的行为 |
| --- | --- |
| 用户输入 | 使用 displayText/skill/combined display 规则和隐藏标志；不导出包裹后的模型输入作为用户原话 |
| Assistant / Thinking | 保存可展示正文，保留 Markdown；thinking 用明确标注的段落/折叠区，不丢弃 |
| 工具调用与结果 | 复用现有工具 kind、状态、摘要、参数展示和结果正文；代码/输出用安全 fence；不补读当前工作区文件充当历史结果 |
| 子 agent 生命周期 | 保存父视图可见的 spawn/finish 信息及子文档链接；完整对话只在 child 文档，避免把 completion receipt 重复成另一份 child 回复 |
| Grow notice、通信、Hook、重试/压缩/回合终态 | 复用已持久或由事实安全重建的会话展示；Hook 仅投影已完成事实，不执行 handler |
| Goal / Workflow / Todo / 后台任务 | 导出会话中可恢复的用户展示/状态，已有截断标记必须保留；不能只忽略 Grow 然后宣称完整 |
| 图片、文件、媒体 | 使用已记录的用户展示引用/说明；Markdown-only 第一版不打包二进制附件、不访问正文里的任意路径，缺失材料明确标记 |
| 不属于对话的 UI | 排除输入框草稿、欢迎页、工具目录、设置、provider 请求证据和仅模型可见上下文；不复现原来的窗口宽度/主题/折叠操作 |

不是每一条历史 transient 都有 durable 事实。能证明原来有内容但材料不可得时，保留 unavailable/truncated 说明；没有证据的内容不制造。正常 full export 与 replay 的最终展示按语义归一化比较，忽略选择、折叠、视窗和动画。

### 4. 输出与下载的所有权

CLI 在输出目录同级创建私有临时目录，逐个节点写入 Markdown，目录和文件采用私有权限；完成所有读取/投影/写入与必要同步后，用不覆盖发布原语提交。发布前任意错误清理临时目录，原有目标不动。进程被强杀可能留下临时目录，但不能留下已经宣称完成的目标树；不保证所有文件系统都支持提交，Unsupported 必须可见。最终 Markdown 总量上限 512 MiB，压缩下载仍受同样的未压缩输出预算约束。

`shell` 不依赖 `pager`。扩展 `trajectory::serve` 的组合参数，由 `pager::trajectory_cmd` 注入窄的导出回调。HTTP 请求只针对该页面绑定的 session；Shell 固定只读来源，worker 内执行回调，返回有所有权的临时下载产物。HTML 下载 `<session-id>.tar.gz`，内部顶层是 `<session-id>/`，下方布局和 CLI 相同。浏览器控制最终下载位置。

沿用 loopback、随机 token、Host 校验和安全响应头。导出整个绑定 session 树，不把当前 layer/search/visibility 过滤器当成导出范围。一次 server 最多一个导出任务在途；响应流拥有临时文件直到完成或断开，断开后释放，禁止在内存中收集无限 tar 字节。RenderBlock 在 worker 内创建并销毁，避免跨线程发送 `!Send` 状态。

### 5. 两个 change 的依赖

本 change 交付来源验证、完整展示事件读取和完整 Markdown 投影；只保留原始可得时间 provenance，不实现播放时钟。`replay-session-transcripts` 消费同一来源与展示 helper，单独引入虚拟时间、流式揭示和只读 TUI。先完成本 change 的验证/归档，再实施后者；后者的 design 引用本 change，归档时维护引用。

## Risks / Trade-offs

- **直接委派与 lifecycle owner 不同**：必须用真实扁平 root-ledger + nested security-parent fixture 验证，单纯三级目录合成测试不足。
- **完整展示涉及分散 handler**：只抽取表格列出的纯展示路径，以 live/offline parity 测试守住边界；不借机执行旧 AgentData 大重构。
- **活跃会话跨 ledger 不一致**：固定每份来源截点并校验必要引用；失败保留来源，提示静止后重试，不宣称全局原子快照。
- **缺失 child / 历史材料**：required child 和损坏事实失败；非权威可选展示材料有明确缺失说明。不能把错误退化为空成功。
- **目录发布的平台差异**：明确使用 no-replace 原语并测试已有空目录/链接；无支持时拒绝，不退化为覆盖。
- **TUI 语义被共享修改带动**：现有 `/export`、`/transcript`、child origin、路径和异步文件队列测试必须保留。

## Validation and handoff

实施验收见 `tasks.md` 与 delta scenarios。第一批 fixture 要覆盖 root、child、grandchild 独有文本和真实扁平 lifecycle；随后补 complete surface parity、缺失/损坏、资源预算、只读源哈希、已有目标、下载流断开。验证正常业务与错误边界，不为每个 helper 机械加镜像测试。

实施及验证记录放在本 change 的 `tasks.md` 和 `verification.md`。只有场景和对应检查真正通过后才勾选实现任务与归档。
