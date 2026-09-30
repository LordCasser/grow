# Replay 只读交互与时间压缩设计

## 1. 范围与基线

这是实施方案；实现与验收结果见 [verification.md](verification.md)。已归档契约以 `openspec/specs/client-surfaces/spec.md` 为准；本 change 的 delta 描述目标。当前工作树含此前 export/replay 以及其他工作的未提交修改，不能通过整文件恢复或批量 reset 清理。

用户明确的范围：没有会话输入框，正常内容仍可点击查看，能进入子 agent；时间使用本机时区；无效间隔用提示和固定短间隔跨过；底部腾出的空间用于 Replay 状态及操作。布局详见 [ux.md](ux.md)，代码草稿状态见 [handoff.md](handoff.md)。

不复用整个 live `App`/`AgentView` 来伪装只读。`mark_as_subagent_view` 只隐藏 composer，并不会移除执行动作。Replay 继续通过 CLI early dispatch 独立启动，只持有来源快照、被动投影、播放时钟和浏览状态。

## 2. 代码边界与最小复用

以下路径均相对 `crates/codegen/`。

| 职责 | 现有入口 | 本次处理 |
| --- | --- | --- |
| CLI 隔离入口 | `cli/src/main.rs`、`pager/src/app/cli.rs` | 保持离线分支，不初始化 provider/SessionActor/ACP client |
| 来源与树验证 | `shell/src/session/storage/transcript.rs::capture_tree_at/TranscriptSnapshot` | 一次固定拓扑、句柄与读取上界；补充来自已捕获 Timeline 的时间依据 |
| 事实到展示 | `pager/src/transcript_projection.rs::TranscriptProjection` | 继续独立处理 turn identity、tool terminal、Behavior/Goal，不混入播放器控制 |
| 调度 | `pager/src/replay_cmd.rs::PlaybackClock/Player` | 时钟移至全树协调层；每节点播放器接受协调时间，不自行读墙钟 |
| 被动正文 | `scrollback/scrollback_pane.rs`、`scrollback/state/` | 复用布局、组、命中、选择框；保留离底阅读位置 |
| 详情 | `views/block_viewer.rs::BlockViewerPane` | 复用纯 entry→viewer 工厂及 raw/data/复制后处理；live runtime 数据留给原调用方 |
| 详情外框 | `views/modal_window.rs` | 复用边框、关闭按钮、鼠标和滚动，限制在正文区域，不遮住播放控制 |
| 正文搜索/选区 | `scrollback/search.rs`、`scrollback/text_selection.rs`、`app/agent_view/selection.rs` | 抽取确有共用需求的被动选区逻辑；不复制整个 AgentView 事件处理器 |
| 底部展示 | `views/status_bar.rs`、`views/shortcuts_bar.rs`、`theme` | 沿用 Theme 与状态/快捷键的视觉语义；Replay 面板自行绘制短字段并维护真实鼠标热区，避免引入 live action registry 或无热区的 ShortcutsBar |

保留已有 `replay_cmd/{browser,tree,time}.rs` 拆分方向即可，不引入通用播放器框架、插件体系或第二套 transcript renderer。底部只需要一个纯布局/绘制单元，输出实际按钮命中矩形；不创建只读 TextArea 或隐藏 composer。

## 3. 固定会话树与可见边界

### 3.1 捕获和准备

1. 校验 speed；调用 `capture_tree_at`。现有节点数 512、深度 32、Timeline 事件 250,000、来源 512 MiB 限额仍统一计数。
2. 在同一快照内逐节点投影事件并抽取活动/等待/恢复时间依据；每个节点只读一次。不因打开子视图再次 `read_session_at` 或查看当前磁盘尾部。
3. 保留 seed 已验证的 lifecycle owner、spawn seq、时间锚点以及实际委派父子关系。lifecycle owner 可以与 UI 中的直接委派父节点不同，不能将二者混成一个 parent ID。
4. 所有必须来源准备成功后才接管终端。准备超过约 300 ms 在原终端 stderr 提供简短阶段提示，不输出历史正文；失败带节点/来源类别并退出，不展示无提示的部分树。准备阶段使用默认 Ctrl-C 退出，不启用 raw mode。
5. 逐步释放已消费的原始 Timeline/ledger 缓存及重复中间正文。既有来源字节限额不等于 RSS 限额；不得同时保留全树 raw JSON、完整 events clone 和最终正文的多份副本。基于实测记录峰值，不声称有未实现的硬内存上限。

### 3.2 播放集合与导航

来源验证遍历捕获树；播放集合则为选中根节点加上其最终有效 transcript 中有可导航子卡片的递归后代。被 rewind 排除且不可达的分支不贡献播放进度、总时长或完成条件；其活动依据在时间重叠时仍可保守阻止把实际工作标成 IDLE，但不显示它的正文/名称。直接传入 child ID 时，该 child 是导航根，不暴露父节点或兄弟正文。

可预先解析播放集合用于总进度，但每个后代的导航入口必须等父视图中对应卡片已实际交付。全树首屏菜单不得枚举尚未出现的子任务。子记录早于 spawn 的异常时间不能使正文先于入口可见。缺少展示 spawn 时间时用已验证 owner Timeline spawn；两者均无可靠时间才用父入口播放位置并标估算，禁止用点击时间或进程当前时间。

每节点保留：事件消费游标、模拟 reveal、TranscriptProjection、viewport/跟随、折叠、选中 entry ID 和本地搜索。导航用路径栈支持嵌套返回。详情/搜索打开时切换导航必须先结束相应局部焦点；返回父节点恢复原阅读位置。只有当前节点构建详情/搜索索引，避免每个隐藏节点都建立一份全文缓存。

### 3.3 一条时钟与有界调度

- 全树一个单调 `PlaybackClock`，状态只有 Playing/Paused/Finished。暂停冻结事件、reveal 和历史 elapsed；Goal paused、turn cancelled 不修改播放器状态。
- 节点内部仍遵从 reader 给出的已验证顺序。节点事件 due time 单调钳制；跨节点合并队首，不按 timestamp 重排各节点全部事实。同值事件用确定性顺序，父 spawn 交付是子入口可见的先决条件。
- 每帧全树最多消费 256 条语义事件，节点轮转/队首调度避免同时间桶的一个大节点占满所有帧。另设约 8 ms 的协作式推进预算，超预算留待下一帧，不丢记录。单条大记录成本需通过压力测试约束，时间预算不是可抢占式硬保证。
- UI 显示时间和百分比使用实际已交付的共同 frontier；若目标时钟领先尚未处理的 due 队列，显示“追赶记录”，不能一边显示 100% 一边仍有文本/子节点未交付。
- Completed 取决于播放集合内事件及 reveal 已耗尽，不取决于历史工具/Goal/Workflow 是否终结。缺终态照旧显示“截至快照未完成”。完成后冻结时间与进度，仍可浏览。
- 仍复用中断边界规则：取消、下一输入等依赖边界到期，先收束旧消息 reveal，再投影边界；不得让近似动画推迟用户打断。

## 4. 历史时间、播放时间与本机时区

区分原始来源位置 S 和压缩后的播放位置 P。`PlaybackClock` 推进 P；一张有序分段映射把 S 映射到 P，绘制历史日期、业务 elapsed 时反向取 S。mode/tool/Goal 的原始时间字段不被重写；倍速只作用 P。没有可证明的原时间使用确定性的相对估算轴，显示“时间估算”，不显示伪造的 1970 日期或 Local::now。缺失/倒退时间段不得自动推导确认 IDLE；逻辑 due 仍单调，原时间字段保留用于详情，混合精度不能让缺失段继承相邻事件的“确切时间”标签。

压缩区间 `[a,b]` 的来源长度 L 对应 1,000 ms 的 P 区间。未压缩区间保持一比一；区间内事件按同一单调映射交付，原顺序和所有语义事件保留。在压缩边界到期前完成依赖 reveal；文本揭示不能跨越已到期的取消/新输入。进度分母包含最终有界 reveal 的播放尾部；空快照直接 Finished，显示“0 条记录”，不做 0/0 除法。

`motion::FrameStamp::at_virtual` 已能分别接收 monotonic elapsed 与 wall_now：动效使用 P，历史 wall_now 使用逆映射的 S。必须审计使用 `frame.now()` 的业务耗时读取点，不能把压缩 P 当成工具真实 elapsed；需要来源耗时的组件传入来源时间，保存的 duration 始终优先，未知则明确未知。暂停时两种时间都冻结。

本机 offset 必须按历史瞬间解析，不能在启动时取一个固定的“现在 offset”用于整个 session。日期格式为 `YYYY-MM-DD HH:mm:ss UTC±HH:MM`；本地为零偏移可显示 UTC。解析不到本机 offset 时回退 UTC；非法/越界时间显示未知。通过注入 resolver 测试 offset 可用、不可用、DST 切换以及夏令时重复小时。已有 `time.rs` 只是初稿，尚未证明本机解析故障分支；不得 catch 后用当前时间掩盖。

## 5. IDLE 和中断空档

### 5.1 默认策略

- **阈值：严格大于 30 秒；压缩间隔：固定 1 秒播放时间。** 30 秒以内不压缩。固定的是倍速作用前的 P 时间，因此 16× 时为 62.5 ms、0.5× 时为 2 秒真实时间。不再引入最低实际停留时间或第二条时钟。
- 跳过时在 Replay 面板保留一行 `跳过 1h 23m 10s IDLE 时间 · 按 1s 重放`，随后改成 `上次跳过 …`，直到下一条跳过提示替换。高倍速也可读，不插入 canonical transcript、不写盘、不参与 export parity。
- duration 表示被压缩的原始区间总长，按秒向下显示，小时不截断；不是把 `L-1s` 伪装成原始时长。原始精度保留在内部映射。
- 空档中 mode/Goal/用量/状态元数据仍按映射交付，不因有心跳就将一段长无活动区间碎成永不触发阈值的小间隔。
- 只考虑来源首尾之间的区间。捕获后到现在、最后已记录事件之后没有可证明终点的时间一律不追加等待；打开但没发过输入且没记录时间的空白不能从文件 mtime 伪造。

### 5.2 判定依据与优先级

优先级：全树已知执行 > 证实的等待 > 恢复空档估算 > 其他未知。空档不是“两条消息相隔很久”；必须使用执行事实的区间并集。确切等待只能从同 owner/对应 operation 的保护区间扣除，不能扣掉并行请求或其他工具；不能关联到具体 operation 的 Plan 状态不覆盖普通工具保护。带恢复标记的对应 operation 不使用完整 reopened duration 作保护，而按第 5.3 节分段；其余操作仍保护。

| 依据 | 判定 |
| --- | --- |
| 正常 Request Started→Completed/Failed/Cancelled | 保护整个已记录请求区间，长推理无输出也不是 IDLE |
| 普通 Tool Started→Completed | 保守保护，因 Started 早于 permission gate；不能直接当已实际执行，也不能直接当等用户 |
| 已证明的 Plan approval_pending→对应清除/完成 | 该等待可作为 IDLE 候选；并行请求、工具、其他后代仍覆盖保护 |
| `ask_user_question` 等交互工具名称、普通权限事件诊断日志 | 单凭工具名称/日志不证明准确等待边界，不自动从普通 tool span 扣除 |
| Hook handler RunStarted→RunFinished | 保护 handler 执行；输入 gate 未响应不等于一直在执行 handler |
| 有可靠起止的后台任务、独立 sideband 模型请求/任务 | 加入全树保护集合；已启动但无终态，保守保护到捕获来源末端 |
| Root 已闲置但 child/嵌套 child 正在运行 | 对所有捕获节点与验证到的 sideband 取并集，禁止仅按当前页面判定 |
| Turn/Step/Goal Active、Workflow Run 尚未 Ended | 状态本身不证明持续计算；已知挂起点可分段。无法判断的独立 Workflow 执行区间保守保护，不只因无模型请求便当 IDLE |
| 没有任何执行或未知活动覆盖的有可靠时间区间 | 已确认 IDLE，按默认阈值压缩 |
| `process_interrupted`、`recovered: true` 等恢复 terminal | 单独估算处理，见下节；不能用恢复时刻计算的 duration 证明进程一直在运行 |
| 缺失/倒退/冲突时间、单纯显示缓存缺记录 | 保持逻辑顺序并标估算；不能据此宣布真实 IDLE |

`PendingInteraction/InteractionResolved` 明确 never persisted；后台 TaskCompleted 通知不总携带输出；sideband 详情当前部分在 reader 校验后释放。实现应在同一捕获读取过程提取所需轻量时间事实，不重新扫描磁盘或扩大到运行时查询。先由测试确认事实来源，不能把表内列出的“可靠时”误当作所有历史都有该字段。

### 5.3 关闭再打开与未知历史

冷恢复会在重开时追加取消/unknown terminal，duration 覆盖离线时间。对带明确恢复标记的跨界操作，最后一个同 owner、已持久化的非恢复活动证据至恢复边界是**中断空档候选**，并不等于精确进程关闭时间。扣除其他节点/sideband 的已知活动后，对剩余超过阈值的部分按 1 秒压缩，显示 `跳过 2h 10m 中断空档（估算） · 按 1s 重放`，不得标成已确认 IDLE。保存原 terminal/outcome、duration 和未完成语义；不合成成功。

没有恢复标记且普通长工具仍开着时，无法仅凭没有输出证明用户离开。默认保护；为避免用户被这种旧记录卡住，提供 `] 下一记录` 的**显式前进**：推进到全树最近一个尚未交付的存储事件（至少推进一条边界），按依赖规则收束 reveal，并显示 `手动跳过 xx 等待时间`；没有确切来源时间则不编造时长。Playing 保持 Playing，Paused 保持 Paused；最终没有下一记录则禁用。不提供任意 seek、倒放或重跑，不以该操作修改历史业务耗时。

本次不新增 durable pending-interaction/process heartbeat 协议来修复旧历史。准确区分所有权限等待与离线区间属于后续持久化契约工作；当前已知限制、保守处理和手动前进都是验收内容，不能在文档中声称完整恢复。

## 6. 浏览与输入边界

普通内容的点击、双击、拖选行为沿用其既有语义；消息文字用于选区，工具卡片/详情入口打开 viewer，子 agent 卡片打开子页。鼠标按下到释放间若发生拖动、滚动、resize、entry 变化则取消点击；按稳定 entry ID 命中，不用过期索引。展开的 verb group 组头与第一个成员占用同 entry 的不同屏幕行，必须分别命中。

raw/detail/search 只能访问截至当前 frontier 已交付的内容，包括尚在 reveal 的消息，不能用队列内完整正文补 viewer。搜索只索引当前节点已显示条目；复制只取当前可见投影。普通正文选区须包含正常渲染的边界与跨行重建，不把“viewer 可以 v 选择”误当“正文拖选已完成”。大段流式更新时，稳定的 entry ID 和选择锚点不跳转；主动上滚停止跟随，End 恢复跟随。

背景任务详情只使用记录内已有输出，缺失则明确提示，不访问运行时任务流/当前 workspace 文件。历史路径、Markdown 链接、图像/图表内容可以显示已保存表示；本次不启动 editor、外部浏览器、native opener、mermaid 子进程或任意命令来补内容。用户明确执行复制可调用已有 clipboard backend，失败可见；它不是 session 执行能力。

输入优先级：Ctrl-C 退出；F8 全局暂停/继续；搜索编辑/详情/帮助局部焦点；Replay 控件点击；正文浏览键。详情/搜索内部的普通字符、Space、`+/-`、q、slash 不泄露到播放器快捷键。Bracketed paste 只送入主动搜索编辑器，其余忽略；不能逐字符分派。Esc 逐层退出查询编辑、搜索结果/选区、详情/帮助、子页；q 在查询编辑内为字符，在非编辑结果/纯选区中先关闭该局部状态，在详情/帮助或子页中返回；根页面没有这些局部状态时 Esc/q 才退出，Ctrl-C 始终只退出播放器。`]` 仅作为正文快捷键，搜索中是字符，详情/帮助不将其向全局冒泡；各焦点下均可显式点击 panel 的下一记录。具体键表见 UX 文档。

## 7. 历史业务状态与底部展示

底部播放状态来自协调器，历史 Behavior/Plan/Goal 来自当前节点已投影事实，两者分区并标明“历史”。Goal clear 后不显示旧目标；新 Goal 替换旧 Goal；late usage 不复活停止状态。退出 Goal 模式但某个子工具还未终结时，保留该工具事实，也不能把 Goal paused 直接判成全树 IDLE。

只显示来源能证明的状态；缺失时写“历史状态未知”，不继承根节点当前状态覆盖 child。根 Goal 已知但 child 无自己的状态时，可在详情中显示明确归属的 root 信息，不能无标签混入 child 状态行。本次不搬 token/context/model 配置按钮、审批队列、自动续跑 CTA；必要历史结果仍在 transcript 内可查。

## 8. 实施和交接

依次完成：修复保留草稿的构建阻塞 → 固定树时间依据/活动分类 → 全局时钟及压缩映射 → 被动浏览与子页 → 底部布局和时区 → 综合验证。该顺序不是要求先写全部功能再测；每步同时增加能暴露对应错误的最小回归。详细任务与验收场景见 [tasks.md](tasks.md)、[audit.md](audit.md)。

当前无待用户批准的产品分歧；本文默认值和降级策略是本次方案的明确选择。遇到事实来源与上述假设不符，应在 change 内记录证据并修订方案，不能静默删掉失败场景或开启 live fallback。
