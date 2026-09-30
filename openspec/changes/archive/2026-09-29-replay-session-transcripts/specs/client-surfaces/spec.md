## ADDED Requirements

### Requirement: CLI replay runs an isolated observational TUI

`grow replay <session-id> [--speed <value>]` SHALL 以指定 session 的固定历史快照启动只读 TUI，默认 speed 为 1。它 SHALL 复用会话展示组件、没有输入框，并且不创建执行 session、连接执行 ACP、调用 provider、运行工具/Hook、恢复 scheduler/workflow 或修改源 session/工作区。必要历史不存在或校验失败 SHALL 明确报错，不进入真实 resume 作为回退。

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
- **WHEN** 用户按 q、Esc 或 Ctrl-C
- **THEN** 只退出播放器并恢复终端，不发送 session cancel 或修改被查看 session。

证据与实施入口：`cli/src/main.rs`、`pager/src/app/cli.rs`、`pager/src/replay_cmd.rs`；复用前序 `export-session-transcript-tree` reader。路径均相对 `crates/codegen/`。

### Requirement: Replay speed and pause use a single virtual clock

播放 SHALL 以单调虚拟时钟调度历史，speed SHALL 是有限且大于零的数值。调速 SHALL 保持当前播放位置连续，暂停 SHALL 冻结事件交付、文本揭示和历史业务耗时。事件顺序 SHALL 按已验证逻辑顺序保持，不以墙钟或 eventId 全量重排；高倍速可以合并绘制，但 SHALL NOT 丢弃语义事件。

#### Scenario: Replay at four times speed
- **WHEN** 两个有可靠间隔的事件相差 8 秒，播放 speed 为 4 且没有暂停
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

### Requirement: Simulated streaming never becomes historical evidence

Replay SHALL 对 assistant 和可见 thinking 提供渐进文本展示，优先使用确切关联的历史时间区间；没有可靠区间时 SHALL 使用确定性的估算并明确标注模拟流式。文本揭示 SHALL 保持 Unicode grapheme 完整及同一消息身份，结束后正文逐字等于 canonical 内容。模拟分片 SHALL 只存在于播放器内存，不写回历史、不参与 sampling admission、不生成原本不存在的工具执行输出。

#### Scenario: Only a complete assistant response was persisted
- **WHEN** 没有可恢复的原始 delta 序列，但存在已接纳的完整回复
- **THEN** 播放器可以分段揭示该回复并标明模拟，最后得到恰好一份完整消息，不声称这是原始 token 到达节奏。

#### Scenario: Unicode and Markdown cross chunk boundaries
- **WHEN** 正文含中文、组合字符、emoji 或 Markdown fence
- **THEN** 揭示不切坏 grapheme，最终文本与原始 Markdown 一致，不产生重复前缀或替换字符。

#### Scenario: Independent event arrives during text reveal
- **WHEN** 文本仍在揭示而独立 Grow/工具展示事件到期
- **THEN** 独立事件可按原逻辑位置应用，后续揭示仍归属于原消息；依赖边界到来时必要正文收束，不为动画重排事实。

#### Scenario: History contains a discarded candidate
- **WHEN** 早期 attempt 被 discarded/quarantined 或回复被 rewind 排除
- **THEN** 该正文不因模拟流式而出现，只有同一只读 reconciliation 允许的内容进入播放。

#### Scenario: Tool history contains only the final result
- **WHEN** 工具结果没有逐段输出事实
- **THEN** 展示保存的结果，不模拟不存在的命令输出、重试或工具执行。

证据与实施入口：`shell/src/session/response_projection.rs::project_admitted_response/plan_response_projections`、`shell/src/session/storage/mod.rs::projection_envelope`、Pager 只读展示投影。路径均相对 `crates/codegen/`。

### Requirement: Replay preserves historical time provenance

播放时间 SHALL 来自与事件明确关联的原通知时间、原写盘时间或 Timeline 事实时间；读取期间合成 projection 的当前时刻 SHALL NOT 被当作历史时间。无法证明时间时 SHALL 标为估算。真实长 idle SHALL 随 speed 缩放，不默认加入额外的无提示 idle 截断。

#### Scenario: Missing response projection is synthesized now
- **WHEN** reader 在本次读取时从 Timeline 重建 response projection
- **THEN** 播放使用其原始 anchor/admission 时间依据或估算，不在历史中插入从原会话到当前时间的巨大空白。

#### Scenario: All source timing is absent
- **WHEN** 合法的展示序列没有可靠时间信息
- **THEN** 按确定性估算轴播放并明确显示估算，不用文件修改时间或当前时间伪装原始节奏。

#### Scenario: History contains a long real pause
- **WHEN** 可靠来源记录一个长 idle 区间
- **THEN** 默认只按指定 speed 缩放该区间，暂停/退出交互继续响应。

证据与实施入口：`shell/src/session/storage/mod.rs::SessionUpdateEnvelope/projection_envelope`、`chat-state/src/timeline.rs::TimelineEvent`、`pager/src/acp/meta.rs`。路径均相对 `crates/codegen/`。

### Requirement: Replay remains browsable and ends without resuming work

Replay SHALL 支持暂停/继续、调速、滚动、选择/折叠和退出，展示播放状态/倍速/进度；没有发送、执行、恢复或授权控件。用户离开底部阅读时 SHALL 保留视窗。播放结束 SHALL 停留在最终页面供阅读；业务事实未结束的节点 SHALL 保留截至快照的未完成/未知状态，不合成成功或继续等待真实执行。

#### Scenario: User scrolls upward during playback
- **WHEN** 用户离开底部浏览已有正文
- **THEN** 后续事件不抢回视窗，回到底部后恢复跟随。

#### Scenario: Snapshot ends during a running tool
- **WHEN** 源截点没有该工具或 turn 的终态
- **THEN** 播放器正常进入 Finished，业务行标明截至快照未完成，不伪造 Completed，也不调用任务查询或等待其真实结束。

#### Scenario: Replay finishes
- **WHEN** 全部快照事件与剩余文本揭示完成
- **THEN** 停在可滚动阅读的最终页面，直到用户退出，不跳转普通交互会话或出现输入框。

#### Scenario: User tries a historical action
- **WHEN** 按 Enter、粘贴文本、slash、权限批准或工具重试快捷键
- **THEN** 不提交任何执行请求；只有明确允许的本地浏览操作可以改变展示。

证据与实施入口：`pager/src/scrollback/scrollback_pane.rs`、`pager/src/replay_cmd.rs` 的只读 keymap/终端页面；禁止使用 `pager/src/app/acp_handler/permissions.rs` 的 live 响应路径。路径均相对 `crates/codegen/`。

### Requirement: Completed replay matches the exported session transcript

在同一来源快照上，播放结束的指定 session 用户展示 SHALL 与完整树导出中该节点 transcript 的语义内容一致，忽略目录导航索引、主题、折叠、视窗和播放控制栏。speed、暂停、绘制合并 SHALL NOT 改变最终消息、工具状态或 Grow 展示。来源读取与播放处理 SHALL 有界，不预建每字符事件队列或随分片无限复制累计正文。

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
