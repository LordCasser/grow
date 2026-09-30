## MODIFIED Requirements

### Requirement: CLI replay runs an isolated observational TUI

`grow replay <session-id> [--speed <value>]` SHALL 以指定 session 的固定历史快照启动只读 TUI，默认 speed 为 1。它 SHALL 复用会话展示组件、没有会话输入框（主动搜索可使用本地搜索编辑器），并且不创建执行 session、连接执行 ACP、调用 provider、运行工具/Hook、恢复 scheduler/workflow 或修改源 session/工作区。必要历史不存在或校验失败 SHALL 明确报错，不进入真实 resume 作为回退。

#### Scenario: Start replay without model configuration
- **WHEN** 历史来源有效，但模型配置无效或没有认证
- **THEN** replay 仍可展示历史，不初始化 provider 或要求修复模型配置。

#### Scenario: Historical execution and interaction appear
- **WHEN** 历史包含工具、Hook、Ask、权限请求或子任务生命周期
- **THEN** 只呈现已记录内容和状态，不重新执行、不发应答、不产生当前权限/任务操作。

#### Scenario: Source cannot be validated
- **WHEN** 指定 session 缺失，或必要 authority/引用损坏
- **THEN** replay 报告来源错误，不新建会话、不恢复执行、不展示无提示的部分成功历史。

#### Scenario: Quit or cancel key is pressed
- **WHEN** 用户在根页面且无局部搜索/选区/详情/帮助时按 q 或 Esc，或在任意播放视图按 Ctrl-C
- **THEN** 只退出播放器并恢复终端，不发送 session cancel 或修改被查看 session；查询编辑中的 q 是字符，Esc 先退出编辑；非编辑搜索结果/纯选区、详情/帮助和子页的 Esc/q 按层级返回。

证据与实施入口：`cli/src/main.rs`、`pager/src/app/cli.rs`、`pager/src/replay_cmd.rs`；复用前序 `export-session-transcript-tree` reader。路径均相对 `crates/codegen/`。

### Requirement: Replay speed and pause use a single virtual clock

播放 SHALL 以单调虚拟时钟调度历史，speed SHALL 是有限且大于零的数值。调速 SHALL 保持当前播放位置连续，暂停 SHALL 冻结自动事件交付、文本揭示和历史业务耗时；只有用户明确选择下一记录时才允许推进，且推进后仍保持暂停。事件顺序 SHALL 按已验证逻辑顺序保持，不以墙钟或 eventId 全量重排；高倍速可以合并绘制，但 SHALL NOT 丢弃语义事件。

#### Scenario: Replay at four times speed
- **WHEN** 两个有可靠间隔的事件相差 8 秒，所在区间没有 IDLE 压缩，播放 speed 为 4 且没有暂停
- **THEN** 播放调度间隔为 2 秒，不因渲染耗时累计漂移；事件内容与顺序不变。

#### Scenario: Pause and resume mid-message
- **WHEN** 用户在文本揭示中暂停，然后恢复
- **THEN** 暂停期间文字、事件游标和历史 elapsed 保持，恢复从原位置继续，不跳过或重复正文。

#### Scenario: Change speed while playing or paused
- **WHEN** 用户修改当前 speed
- **THEN** 当前虚拟位置不跳变，后续间隔按新速度计算；暂停时仍保持暂停。

#### Scenario: Invalid speed
- **WHEN** speed 为零、负值、NaN、无穷或不能安全换算的数值
- **THEN** 在终端接管前报告参数错误，不启动播放。

#### Scenario: Timestamps regress or event counters restart
- **WHEN** 历史墙钟倒退、时间精度导致同值，或 resume 后事件 counter 重新起算
- **THEN** 播放保持已验证的事件顺序，等待时间非负，不把旧记录重排为新的因果顺序。

证据与实施入口：`pager/src/motion.rs::FrameStamp`、`pager/src/acp/meta.rs::NotificationMeta`、`pager/src/replay_cmd.rs::PlaybackClock/Player`。路径均相对 `crates/codegen/`。

### Requirement: Replay preserves historical time provenance

播放时间 SHALL 来自与事件明确关联的原通知时间、原写盘时间或 Timeline 事实时间；读取期间合成 projection 的当前时刻 SHALL NOT 被当作历史时间。无法证明时间时 SHALL 标为估算。历史来源轴与播放轴 SHALL 分离；已确认长 IDLE 按下述固定区间规则压缩并提示，倍速作用于播放轴，不改写原始时间或业务 duration。

#### Scenario: Missing response projection is synthesized now
- **WHEN** reader 在本次读取时从 Timeline 重建 response projection
- **THEN** 播放使用其原始 anchor/admission 时间依据或估算，不在历史中插入从原会话到当前时间的巨大空白。

#### Scenario: All source timing is absent
- **WHEN** 合法的展示序列没有可靠时间信息
- **THEN** 按确定性估算轴播放并明确显示估算，不用文件修改时间或当前时间伪装原始节奏。

#### Scenario: History contains a long real pause
- **WHEN** 可靠来源记录严格超过 30 秒且全树没有执行/未知活动保护的 IDLE 区间
- **THEN** 默认压缩为 1 秒播放时间并在面板显示原长度和 IDLE 提示；再按指定 speed 缩放，暂停/退出保持响应。

证据与实施入口：`shell/src/session/storage/mod.rs::SessionUpdateEnvelope/projection_envelope`、`chat-state/src/timeline.rs::TimelineEvent`、`pager/src/acp/meta.rs`。路径均相对 `crates/codegen/`。

### Requirement: Completed replay matches the exported session transcript

在同一来源快照上，播放结束的指定 session 用户展示 SHALL 与完整树导出中该节点 transcript 的语义内容一致，忽略目录导航索引、主题、折叠、视窗、播放控制栏和仅属于播放器的跳过提示。speed、暂停、IDLE 压缩、显式下一记录、绘制合并 SHALL NOT 改变最终消息、工具状态或 Grow 展示。来源读取与播放处理 SHALL 有界，不预建每字符事件队列或随分片无限复制累计正文。

#### Scenario: Compare different playback schedules
- **WHEN** 同一来源以 1 倍、4 倍及多次暂停/调速播放完成
- **THEN** 三者的归一化展示与该节点完整 Markdown 的内容/状态一致。

#### Scenario: Replay a child session
- **WHEN** 指定一个 child session ID
- **THEN** 播放该 child 的 transcript，与导出树中对应节点一致，不混入其兄弟正文或重新启动 child。

#### Scenario: Large transcript and slow terminal
- **WHEN** 大正文、高倍速和慢绘制同时出现
- **THEN** 可以合并到期绘制，所有语义事件仍恰好应用，退出/暂停保持有界响应，源预算超限时明确失败。

证据与实施入口：前序 `export-session-transcript-tree` 的完整投影/reader、`pager/src/acp/tracker.rs`、`pager/src/scrollback/state/` 及新增 replay 测试。路径均相对 `crates/codegen/`。

#### Scenario: Complete the navigable captured tree
- **WHEN** 父节点和最终有效 transcript 中可导航的全部后代播放完成
- **THEN** 每个节点分别与同一快照导出一致；切页不混合正文，被 rewind 排除的不可达分支不延长完成条件。

### Requirement: Replay semantic boundaries take precedence over simulated text

到期的依赖事件 SHALL 收束此前模拟正文并立即按既有逻辑顺序交付；模拟动画 SHALL NOT 推迟用户打断、下一输入或工具边界。暂停 SHALL 同时冻结自动事件交付与模拟正文，用户明确下一记录除外。终端粘贴 SHALL 被视为整体输入；只有主动搜索编辑器可以消费为查询，其他场景忽略，不触发播放器快捷键或会话提交。

#### Scenario: Cancellation arrives during reveal
- **WHEN** 文本仍在模拟揭示，而取消 terminal 或新用户输入已到期
- **THEN** 先收束已记录正文，再在本 tick 交付到期边界；下一消息不继承前一 reveal。

#### Scenario: Paste contains playback keys
- **WHEN** 粘贴的正文含 q、空格、加减号或 slash command
- **THEN** 不退出、不调速、不暂停、不执行命令；主动搜索只将其作为查询，其他场景忽略；独立键盘快捷键仍可浏览或退出。

## ADDED Requirements

### Requirement: Replay supports ordinary read-only transcript inspection

Replay SHALL 复用普通会话被动浏览组件，支持鼠标与键盘选择、分组折叠、详情、搜索、正文/详情文本选择与复制。浏览 SHALL 仅使用已交付的投影，不访问未来正文、当前执行数据或未捕获外部文件。它 SHALL 不提供会话提交、修改历史、执行工具或批准交互的能力；历史外链、文件或图表 SHALL NOT 触发浏览器、editor、native opener、Mermaid 子进程或其他执行补全。仅用户明确复制时允许调用既有 clipboard backend，不允许历史内容提供待执行命令。

#### Scenario: Inspect a recorded tool or message
- **WHEN** 用户操作已显示内容的详情入口或选择后按 Enter/Ctrl-F
- **THEN** 打开相应查看器，可以搜索/选择/复制当前已交付内容；关闭回原阅读位置，不执行记录中的命令，也不提前展示 reveal 尚未交付的文本。

#### Scenario: Expand grouped rows
- **WHEN** 用户点击分组头或对选中组按 Enter，再查看已展开成员
- **THEN** 正确展开/折叠和打开成员详情；共享 entry 的组头不会误打开第一个成员。

#### Scenario: Select text while records arrive
- **WHEN** 用户拖选正文或详情文本，同时新记录到达、窗口调整或鼠标释放
- **THEN** 已有选区按稳定 entry 身份保持；拖动和失效热区不触发点击，复制失败有可见反馈。

#### Scenario: Local input owns its keys
- **WHEN** 用户在搜索或详情内输入字符、空格、q 或加减号
- **THEN** 优先遵循当前局部控件行为，不触发播放快捷键；F8 可控制播放，Ctrl-C 只退出播放器。

#### Scenario: Close nested inspection
- **WHEN** 用户在搜索编辑/选区、详情/帮助或子 agent 页面按 Esc
- **THEN** 逐层退出查询编辑、结果/选区、详情/帮助、子页，根页面才退出；q 在查询编辑内是字符，在其他局部层按同样层级返回；返回保持阅读位置和播放状态。

#### Scenario: Late search result belongs to an old view
- **WHEN** 关闭搜索、修改查询或切换子页后旧后台结果到达
- **THEN** 旧 owner/generation 的结果不抢焦点或滚动位置，查询不包含未来队列。

#### Scenario: Historical auxiliary output is absent
- **WHEN** 记录中的后台任务输出、媒体或外部文件内容无法从已捕获历史获得
- **THEN** 显示可用表示或明确缺失说明，不连接当前任务、不启动命令或读取当前 workspace 文件冒充历史；点击历史链接/媒体不启动 browser/opener/renderer 进程。

### Requirement: Replay navigates a captured agent tree on one clock

Replay SHALL 一次验证和读取固定会话树，最终有效 transcript 中的可导航后代与选中根共用播放时钟，各自保存阅读状态。跨节点 SHALL 不运行 session、不重新捕获当前来源、不提前展示未来子任务。生命周期 owner 锚点与委派父子导航 SHALL 保持各自归属。

#### Scenario: Open a subagent during playback
- **WHEN** 对应子 agent 卡片已经交付，用户打开该节点或其已出现的后代
- **THEN** 展示截至当前已交付播放位置的过程；暂停/调速应用于整棵播放树，切页不重置时间，返回恢复阅读位置。

#### Scenario: Child time is missing or precedes spawn
- **WHEN** 子事件没有可靠时间，或其时间早于父入口
- **THEN** 依据已验证 owner spawn 作因果锚定，缺失则明确估算；不能使用点击时间，父卡片交付前不可导航到未来正文。

#### Scenario: Browse after completion
- **WHEN** 播放集合内各节点事件及 reveal 均已完成
- **THEN** 页面停留可浏览；历史未结束工具或 Goal 保持截点状态，不继续等待真实执行。

#### Scenario: Child source is invalid or unavailable
- **WHEN** 必须的子节点来源缺失、损坏、无法验证或超出来源预算
- **THEN** 接管终端前明确失败，不连接 live agent，不展示无提示的不完整树。

#### Scenario: Large tree is due at high speed
- **WHEN** 多节点同时有大量到期事件
- **THEN** 全树共享有界推进预算并避免单节点长期独占，输入保持响应；显示进度以已交付 frontier 为依据，不提前宣告完成。

### Requirement: Replay panel replaces live composer and status controls

Replay SHALL 在正文下方使用专用状态/操作面板，替代 composer 及执行会话状态控件。面板 SHALL 显示播放状态、倍速、全局进度、当前节点上下文、历史时间和当前焦点可用操作；不提供配置、发送、停止历史任务、审批或续跑动作。

#### Scenario: Standard terminal
- **WHEN** 终端至少 100 列、24 行
- **THEN** 面板最多占 5 行，按上下文、播放控制、时间、历史状态/跳过提示、快捷键组织；正文占剩余空间，不保留空 composer。

#### Scenario: Narrow or short terminal
- **WHEN** 窗口缩小或含中文长标题
- **THEN** 按 cell 宽度和可用行数降级，优先保留播放状态及可发现的帮助/退出；完整被省略信息在只读帮助可查，无越界或过期鼠标热区。

#### Scenario: Playback focus changes
- **WHEN** 用户打开详情、搜索、子页或播放进入 Paused/Finished
- **THEN** 提示与当前实际动作一致；无效动作禁用，状态不只靠颜色表达，详情不遮住播放控制。

#### Scenario: Reading away from the bottom
- **WHEN** 用户滚离底部或打开详情
- **THEN** 不隐式暂停也不抢回阅读位置；面板明确跟随状态，End/跟随控件只改 viewport。

#### Scenario: Playback and historical state differ
- **WHEN** 播放器 Playing 而历史 Goal paused、已退出 Goal 模式或存在未完成工具
- **THEN** 播放状态与历史业务状态分开显示，不从任一状态推导另一状态，不发起恢复/继续。

### Requirement: Replay displays recorded time in the local timezone

Replay SHALL 使用本机在记录瞬间的时区显示历史时间并标明 offset；无法获取时 SHALL 回退 UTC。时区只影响展示，不改变事件顺序/间隔。缺少可靠来源时间或非法时间 SHALL 显示估算/未知，不使用当前时间伪装。

#### Scenario: Local timezone is available
- **WHEN** 记录瞬间本机 offset 为 UTC+08:00
- **THEN** 显示对应本地日期及 offset，来源轴、播放轴、倍速不变。

#### Scenario: Local offset changes with historical daylight saving
- **WHEN** 历史跨越本机时区 DST 变化或重复小时
- **THEN** 每个瞬间使用相应 offset，绝对顺序和播放间隔保持不变。

#### Scenario: Local timezone is unavailable
- **WHEN** 无法解析记录瞬间的本机 offset
- **THEN** 对合法绝对时间显示 UTC 并明确标为 UTC；非法绝对时间显示未知，不猜测偏移或用 now 回填。

### Requirement: Replay compresses only supported idle and recovery gaps

Replay SHALL 基于捕获树的执行与等待事实分类区间，已确认 IDLE 严格超过 30 秒时压缩为固定 1 秒播放时间。该时间随后受 speed 控制。所有节点、已验证 sideband 与后台执行的已知或无法排除的活动 SHALL 阻止普通 IDLE 压缩；单纯没有输出、Goal paused 或 turn 结束 SHALL NOT 证明全树空闲。中断恢复空档可估算压缩，但 SHALL 与确认 IDLE 明确区分。

#### Scenario: Skip a confirmed long idle
- **WHEN** 可靠区间内全树无执行/未知活动保护且时长超过 30 秒
- **THEN** 按 1 秒播放时间跨过，并显示一行“跳过 xx IDLE 时间”及固定播放间隔；随后保留上次跳过说明，高倍速不让提示只闪一帧。

#### Scenario: Threshold and speed apply predictably
- **WHEN** IDLE 为 30 秒、30 秒加 1 毫秒，或对被压缩区间使用 0.5×/16×
- **THEN** 30 秒不压缩，超过阈值压缩；1 秒播放间隔分别对应 2 秒/62.5 毫秒实际调度间隔，不额外添加不可暂停的墙钟等待。

#### Scenario: Another agent remains active
- **WHEN** 当前页面无输出但子节点、sideband、后台任务或其他已知执行覆盖该区间
- **THEN** 对重叠部分不标为 IDLE，切换当前页面不改变分类。

#### Scenario: Permission wait cannot be proven
- **WHEN** 只有跨越长时间的普通 Tool Started/Completed，没有可关联的人工等待边界
- **THEN** 保守保留，不能凭工具名或无输出将全程标成 IDLE；允许用户显式前进下一记录。

#### Scenario: Session recovers after an interruption
- **WHEN** 存在明确恢复 terminal，最后可信活动至恢复边界之间有长空档
- **THEN** 扣除其他已知活动后可压缩估算空档，提示“中断空档（估算）”；不把恢复 duration 当成确切运行时间或确切 IDLE，不改变 terminal/outcome。

#### Scenario: Metadata changes inside idle
- **WHEN** 可压缩空档内有多次模式、Goal、用量或其他展示元数据变化
- **THEN** 所有事件沿单调映射按原逻辑顺序交付，不丢事件，也不因元数据心跳把一段空闲拆成永不压缩的小间隔。

#### Scenario: Snapshot has no recorded trailing endpoint
- **WHEN** 最后记录后没有可靠末端，或 session 打开未输入期间没有记录时间
- **THEN** 不用 now/mtime 添加等待或计算虚构 IDLE，现有快照消费完即结束。

### Requirement: Explicit replay advance remains observational

Replay SHALL 提供显式下一记录操作，以跨过不能自动判断的等待。它 SHALL 只推进全树播放位置到最近未交付的记录边界，保持原事件顺序与当前 Playing/Paused 意图，不改历史业务时长，不执行任何任务。正文 `]` 与显式 panel 点击可触发该操作；搜索编辑中的 `]` 只是查询字符，详情/帮助不将此键冒泡为全局推进。

#### Scenario: Advance while paused
- **WHEN** 用户暂停中选择下一记录
- **THEN** 依赖 reveal 正确收束、至少交付下一条边界后仍暂停；提示手动跨过的等待，未知时长不猜测，最终内容不变。

#### Scenario: Advance while playing
- **WHEN** 用户在 Playing 时从正文快捷键或任意局部焦点下显式点击下一记录
- **THEN** 推进至最近待交付边界后继续 Playing，保留原局部焦点；不发起任何历史动作。

#### Scenario: Advance key belongs to local search
- **WHEN** 搜索编辑器收到 `]` 字符，或详情/帮助接到该键
- **THEN** 查询正常编辑，详情/帮助遵循自身键位且不向全局冒泡；播放游标不因此前进。

#### Scenario: No remaining record
- **WHEN** 全部事件及 reveal 已完成
- **THEN** 下一记录禁用，页面继续可浏览，不重启或恢复 session。
