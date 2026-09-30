## Context

共同的代码证据和已确认范围见 [前序分析](../archive/2026-09-29-export-session-transcript-tree/analysis.md)。前序 change 完成数据来源与完整用户展示；本 change 不再次实现 Timeline → transcript 规则。

关键已有结构：

- `cli/src/main.rs` 的 early command dispatch 可直接进入离线命令，不需要创建 `AgentConfig` 或执行 session。
- `pager/src/scrollback/scrollback_pane.rs`、`scrollback/state/`、`scrollback/block.rs` 已有会话条目与渲染；`views/agent.rs::AgentViewLayout` 是纯布局辅助。
- `pager/src/app/mod.rs::run` 是连接/恢复执行入口；`app/acp_handler/permissions.rs` 等路径可能回复请求，不能作为播放器入口。
- `NotificationMeta.is_replay` 当前表示恢复历史，会抑制部分 transient UI；`motion::FrameStamp` 和 `SubagentInfo::display_elapsed_at` 有可注入时间的先例。

以上路径相对 `crates/codegen/`，实际实现前仍需核对工作树，不能以旧 Atlas 行号直接编辑。

## Goals / Non-Goals

**Goals**：历史内容与完整 transcript 相同；播放速度、暂停与近似流式可控；复用会话渲染；运行能力从结构上限制为历史读取和终端展示。

**Non-Goals**：启动/继续真实 session；精确恢复 token 的原始到达时间；回放已丢弃候选正文；录屏级主题/鼠标/折叠复原；修改源文件；长会话 seek/checkpoint；多个 session 同步播放；播放器内恢复 agent 或执行历史链接动作。

第一版播放指定 session 自身的会话展示，并呈现它已有的子 agent 生命周期。需要观看 child 正文时使用 `grow replay <child-session-id>`；不在本 change 增加整棵树的同步浏览调度。对照导出时比较该节点 transcript，子文档索引是导航元数据，不是额外的历史消息。

## Decisions

### 1. 入口与能力边界

```text
grow replay <session-id>                 # 1×
grow replay <session-id> --speed 4        # 4×
```

参数要求 speed 为有限且大于 0 的数值，错误在进入 raw/alternate screen 前报告。来源读取/校验失败也优先在终端接管前报告。不依赖模型、认证或当前 session 工作目录仍然存在；只读取显示所需的有效 UI 配置，损坏模型配置不能阻断离线播放。

在 `pager` 内增加窄的 replay 命令/页面，复用 terminal 生命周期和会话 widgets；不进入普通 `app::run`，不创建 ACP agent transport，不发送 initialize/session/load/prompt，也不构建执行 actor、sampler、task coordinator 或 scheduler。不要创建假的 transport 再依靠所有请求都被忽略来实现只读。

页面持有现有展示状态、播放时钟、事件/文本揭示游标和终端交互状态。共享展示转换由前序 change 提供；只提取必要 helper，不借本次播放器实现 AgentView 的完整 data/view 重组。

### 2. 快照、顺序和来源时间

复用前序 reader 的 pinned 来源与已提交截点。播放期间不读取新增尾部、不补做 crash repair，也不根据工具名、文本或相邻 request 猜测缺失内容。source authority 冲突、必要材料缺失或预算超限直接失败。

调用单 session 读取单元，不运行完整导出树的正文遍历。播放父级生命周期只消费该节点已有的展示/事实；验证当前节点需要的 owner 引用时只加载必要材料，不因未请求的兄弟或 child transcript 缺失而阻断当前节点的播放。

每条派生展示事件保留 session、来源位置/事实引用、稳定逻辑顺序和可选时间证据。排序继续使用已验证的 replay 因果顺序；不按 `eventId` 数字或墙钟全量重排，因为跨 resume ID 可重新起算，独立事件也可能交错。

时间选择使用与该事件确切关联的原始通知 `agentTimestampMs`，无该值时使用原始写盘时间或确切 Timeline 事实时间。response 内容合成时使用原 candidate anchor / admission 事实的明确关联，不使用重新生成 envelope 的 now。所有降精度/缺失时间保留 provenance。已有时间倒退时展示调度使用单调钳制，不改变事件顺序、不产生负等待，也不修改原始历史字段。

已知暂停按原间隔参与播放，不默认截断长 idle；避免给 `--speed 4` 混入不透明的额外压缩语义。首版不增加 idle-cap/skip-idle 选项。来源完全无时间时，以确定性估算形成播放轴，界面明确它是估算轴。

### 3. 一个虚拟播放时钟

播放状态只需 `Playing`、`Paused`、`Finished`；读取错误在进入播放前失败，运行期显示/读取错误结束播放并走统一终端恢复。

```text
virtual_now = virtual_anchor + (monotonic_now - wall_anchor) * speed
```

暂停时先结算 virtual_now，再冻结；恢复重新设置 wall_anchor。调速也先按旧 speed 结算，再更换 anchor 和 speed。不能为每个事件直接做长 sleep，更不能依次 sleep 后累积绘制耗时。event loop 等待最近到期事件、短帧截止或按键；高倍速合并到同一帧的到期更新，但不能丢语义事件。

历史业务时间从虚拟轴派生，暂停后 turn/tool/child elapsed 也冻结，终态使用已记录 duration。终端光标/鼠标等纯交互反馈可用正常 UI 时钟，不反向推进历史。现有 motion 接口如果把 wall_now 与动画时间绑定，需要增加最小可注入展示样本；不能拿 `FrameStamp::at` 的 UNIX_EPOCH 假值展示成历史日期。

### 4. 近似流式只改变揭示过程

第一版为 assistant 和可见 thinking 模拟渐进文本。保留了真实工具输出更新的部分按这些更新播放；只有最终工具结果时一次出现，不合成不存在的 shell 输出或参数执行过程。

优先利用有证据的 stream-start、candidate anchor、admission/完成边界形成可用区间。它们描述的是记录时刻，不声称逐 token 到达时间。可用正区间内按 Unicode grapheme 边界均匀揭示；只有无可靠区间时使用固定默认策略（建议 60 grapheme/s、每段 0.2–4 秒）形成明确标注的估算时长。该策略是 renderer 的常量与测试基线，不新增用户配置体系。

揭示游标绑定到来源消息/展示条目的稳定 identity，后续分片更新同一条目，不把每个字变成新消息。保留完整 canonical 正文，虚拟分片只在内存中表示“当前显示到哪里”；不能反向写回 Timeline/updates 或冒充新的 sampling attempt。

存在独立 Grow/工具事件时继续按其逻辑位置和到期时间应用，不能让大段文本的动画阻塞这些更新。进入同消息的替换、后续同通道消息或依赖该回复的工具边界前，结束必要的文本揭示，保证事件因果和最终正文。高倍速可合并剩余揭示到当前帧；同时间戳或缺少顺序以外的证据时允许瞬间收束，禁止为了动画交换两条事实的顺序。缺失时间产生的估算区间只延后必要的展示调度并标明估算，不篡改历史 timestamp。

以上规则不是新通用调度框架：单一事件游标、稳定条目上的少量 reveal 游标和一个时钟即可。现有 tracker 对 live attempt 的撤回/接受规则继续保留；播放器不把 synthetic reveal 送入 live attempt 准入路径。

### 5. 页面与交互

布局复用已有会话正文/工具样式，移除 prompt 区域及其占位空间，底部为 `Replay · 模拟流式 · 4× · 播放进度` 与控制提示。没有新建/发送、slash、rewind执行、retry、resume、kill、Apply diff 或权限批准入口。

- Space：暂停/继续；`+` / `-` 调速，显示实际倍速；命令行设置可为任意合法正有限值。
- 既有滚动/选择/折叠键和鼠标浏览：只改变视窗，不更改历史数据。离开底部后不被自动滚动夺回；回到底部恢复跟随。
- `q` / Esc / Ctrl-C：退出播放器并恢复终端；Ctrl-C 不发送 session cancel。
- 到达末尾：状态改为 Finished，停在最终页面，可继续滚动阅读，再由用户退出。

Ask、permission、Plan approval 等按已保存状态展示；没有应答通道，不能重新打开交互。若快照结束时某工具/turn 仍未结束，保留“截至快照仍进行中/结果未知”的状态，不合成完成，不无限等待。UI loading/播放状态与原 session 的业务运行状态分开。

图片/媒体只展示可安全读取的已记录内容或已有占位；失效引用显示 unavailable。历史链接不触发工具/命令；首版不启动外部浏览器、编辑器或 Mermaid 子进程来执行历史操作，可显示源码/文本回退。终端初始化/绘制/恢复是播放器本身的正常行为，不属于 session 执行。

### 6. 有界处理与同一最终展示

来源沿用前序 reader 的预算；按事件迭代、按当前可见文本揭示，不预建每字符事件数组，不为每个分片复制累计全文。大工具输出沿现有展示截断/详情策略并明确标记；不引入无界缓存。慢终端下绘制可以合并，但暂停、退出和调速仍要在下一有界事件循环切片响应。

比较完整导出与播放完成的同 session 语义结果：用户/assistant/thinking 正文、工具身份/状态/结果、已恢复的 Grow 展示一致；排除目录导航索引、折叠、视窗、主题和播放控制栏。倍速、暂停次数和绘制合并不能改变结果。

## Risks / Trade-offs

- **没有原始 delta timing**：始终标明模拟流式，缺时用确定性估算；不通过新增记录机制扩展本次范围。
- **`isReplay` 会抑制 live UI**：按历史展示投影驱动播放状态，不能切成 live 通知或复用有请求回复的总 handler。
- **旧 view 内隐式 now / 副作用**：逐项检查复用 helper，业务时间显式传入，no-execution 验证覆盖快捷键、权限、Hook、任务终态和退出路径。
- **极端倍速与相同时间戳**：可合并绘制/文本揭示但不丢事实；时长计算溢出或无法表示时拒绝参数，不能 panic。
- **父子多时间轴**：首版一次播放一个 session，父生命周期照常展示；同步跨 agent 播放留待独立需求，不埋入当前调度器。

## Validation and handoff

使用假单调时钟验证播放与暂停/调速，避免墙钟 sleep 的脆弱测试；使用生产 reader/展示 helper 的 fixture 验证内容；用真实 PTY 验证无输入框、控制响应与终端恢复。provider/工具/Hook 使用可计数或遇调用即失败的测试桩，源目录与工作区前后哈希比较，证明只读能力边界。

实施与验证记录位于本 change 的 `tasks.md` 和 `verification.md`；归档前不将 delta 提前写入主规范。
