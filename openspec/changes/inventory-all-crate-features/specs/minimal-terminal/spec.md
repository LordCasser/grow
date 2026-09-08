## ADDED Requirements

### Requirement: Minimal renderer registration
install SHALL 通过 pager minimal_hook 注册 draw 函数，使宿主调用本包渲染而不建立反向 Cargo 依赖。

#### Scenario: 实现边界
- **WHEN** 宿主未安装 hook
- **THEN** 本包不自行接管终端；重复安装处理属于 pager 接缝。

证据：`crates/codegen/pager-minimal/src/lib.rs` — `install`。

### Requirement: Minimal frame execution order
draw SHALL 依次同步 pending marks、推进 transcript、welcome、plan、准备 tail 模式、调整 viewport、提交、展开和绘制 live；提交失败后重新调整 viewport。

#### Scenario: 实现边界
- **WHEN** 开始同步更新或 autoresize 失败
- **THEN** 本层忽略这些错误；不能由帧顺序推导所有终端 IO 都成功。

证据：`crates/codegen/pager-minimal/src/lib.rs` — `draw`。

### Requirement: Minimal ordered commit frontier
commit_leading_run SHALL 从 scan cursor 顺序推进，跳过已提交 ID，在第一个不可提交或失败项停止，仅回调成功后标记并前移。

#### Scenario: 实现边界
- **WHEN** 终端已部分写入但回调失败
- **THEN** 保留当前项重试，不保证底层 IO 事务性或绝无重复字节。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `commit_leading_run`。

### Requirement: Minimal pending input and coordination hold
is_committable SHALL 始终保留 pending user input 和未 terminal coordination 行，即使主 turn 已空闲。

#### Scenario: 实现边界
- **WHEN** 后方已有完成条目
- **THEN** 仍不能越过阻塞项；coordination terminal 后才继续。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `is_committable`。

### Requirement: Minimal running entry exceptions
运行中提交 SHALL 允许非 running 条目、BgTask 和后方已有条目的 running AgentMessage；其它 running 条目保持 live。

#### Scenario: 实现边界
- **WHEN** turn 空闲但条目遗留 running
- **THEN** 除输入和 coordination 阻塞外允许提交；生产回调先 finish_running。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `is_committable`。

### Requirement: Minimal shared frontier prediction
scan_frontier SHALL 复用提交分类输出 tail_start 和 will_commit，不修改提交标记；failed frontier 阻止同帧再次预测成功。

#### Scenario: 实现边界
- **WHEN** 开始下一次提交或准备新帧
- **THEN** 失败标记清除并允许重试；预测不是写入成功证明。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `scan_frontier`。

### Requirement: Minimal print display policy
minimal_commit_display_mode SHALL 保留 Command notice 默认模式、折叠 coordination、展开 Edit、折叠成功查询类，其余工具截断，Thinking 按开关，其余展开。

#### Scenario: 实现边界
- **WHEN** 查询失败
- **THEN** 保持 Truncated 而不是成功查询的 Collapsed；模式在测量 live tail 前应用。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `minimal_commit_display_mode`。

### Requirement: Minimal shared block appearance
minimal_renderer SHALL 为 live 与 committed 使用同一布局构造，传入所属 cwd 和 frame、平坦背景，仅未折叠 Thinking 保留 dim accent。

#### Scenario: 实现边界
- **WHEN** 构造 committed appearance
- **THEN** 克隆配置并关闭时间戳、左右 padding，启用 thinking dim/italic 和折叠展开提示。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `minimal_renderer`。

### Requirement: Minimal capped native output
insert_committed SHALL 按完整 desired_height 布局，以非零 max_rows 限制提交 buffer，高度为零直接成功；超限末行显示隐藏行数和 transcript 提示。

#### Scenario: 实现边界
- **WHEN** max_rows 为零
- **THEN** 不截断；限制 buffer 不保证完整布局及链接计算开销有界。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `insert_committed`。

### Requirement: Minimal capped hyperlink coverage
提交链接 SHALL 遵循 hyperlink route，并在截断时排除 footer 行及以下的语义链接。

#### Scenario: 实现边界
- **WHEN** footer 覆盖原内容行
- **THEN** 不保留该行原链接；footer 使用 Reset 背景和 dim 样式。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `insert_committed`。

### Requirement: Minimal commit hold and failure mutation
commit_active SHALL 在无 agent、零宽、app modal 或 session reload 时暂停；写入成功后才记录可展开项。

#### Scenario: 实现边界
- **WHEN** 写入失败
- **THEN** 提交标记不前移，但 finish_running 与 display mode 不回滚；返回失败前沿是否存在。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `commit_active`。

### Requirement: Minimal explicit expansion replay
expand_pending SHALL guard 通过后消费队列，以 Expanded 无 cap 重印；不存在的 ID 跳过，失败当前及余下 ID 重新排队。

#### Scenario: 实现边界
- **WHEN** 旧内容已经进入 native history
- **THEN** 展开添加新副本，不原地修改旧终端内容；失败也不回滚 entry 模式。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `expand_pending`。

### Requirement: Minimal pending mark synchronization
sync_pending_marks SHALL 在 active agent 上调用宿主 pending-input 同步，并在 viewport 测量之前运行。

#### Scenario: 实现边界
- **WHEN** 本帧刚收到 permission 或 question
- **THEN** 测量与提交读取相同标记，不把等待输入的工具提前提交。

证据：`crates/codegen/pager-minimal/src/commit.rs` — `sync_pending_marks`。

### Requirement: Minimal welcome retry
maybe_commit_welcome SHALL 在 pending 且宽至少8时打印版本、可选cwd/model和help卡片，成功才清 pending。

#### Scenario: 实现边界
- **WHEN** insert_before 失败
- **THEN** 下帧重试；此前 clear 错误忽略，不保证部分终端写入可回滚。

证据：`crates/codegen/pager-minimal/src/welcome.rs` — `maybe_commit_welcome`。

### Requirement: Minimal startup trust presentation
render_startup SHALL 根据 TrustState 显示信任提示或 Starting，过滤路径控制字符并按字符计数换行。

#### Scenario: 实现边界
- **WHEN** 路径含宽字符或组合字素
- **THEN** 当前计算不是 terminal cell/grapheme 算法；此函数只显示，不作信任决定。

证据：`crates/codegen/pager-minimal/src/startup.rs` — `render_startup`。

### Requirement: Minimal plan insertion identity
maybe_commit_plan SHALL 按当前审批 tool_call_id 去重，将计划消息插到对应 pending tool 前，找不到 anchor 则追加。

#### Scenario: 实现边界
- **WHEN** 消息已插入但终端写失败
- **THEN** 去重 ID 表示模型入列，不表示 native 输出或持久化完成。

证据：`crates/codegen/pager-minimal/src/plan.rs` — `maybe_commit_plan`。

### Requirement: Minimal plan approval controls
plan render SHALL 按 Preview、Commenting、Prompt 呈现不同提示，Prompt 下有评论或非空反馈时提示 request changes，否则 approve。

#### Scenario: 实现边界
- **WHEN** 绘制审批控件
- **THEN** 仅产生显示及输入 cursor；实际批准与按键分派由宿主处理。

证据：`crates/codegen/pager-minimal/src/plan.rs` — `render`。

### Requirement: Minimal todo visibility
todo_panel_visible SHALL 对非空列表在 force 或存在 Pending/InProgress 时显示，默认高度最多8行，force 使用条目数再由布局限高。

#### Scenario: 实现边界
- **WHEN** 所有任务完成且未 force
- **THEN** 隐藏，与 turn 是否运行无关。

证据：`crates/codegen/pager-minimal/src/todo.rs` — `todo_panel_visible`。

### Requirement: Minimal todo row rendering
todo_panel_lines SHALL 保持顺序，以不同状态 glyph/style 显示首行 trim 后最多64字符，并为溢出保留最后一行计数提示。

#### Scenario: 实现边界
- **WHEN** max_rows 为零或字符包含组合字素
- **THEN** 零行返回空；字符截断不保证 grapheme/cell 安全。

证据：`crates/codegen/pager-minimal/src/todo.rs` — `todo_panel_lines`。

### Requirement: Minimal list panel selection
panel active SHALL 优先 Resume，再 MCP tab，使用共享 picker 内容和搜索渲染；高度为4行 chrome 加 body 且最低5。

#### Scenario: 实现边界
- **WHEN** 传入 ceiling 小于5
- **THEN** 仍可能返回5，极窄高绘制边界依赖调用布局。

证据：`crates/codegen/pager-minimal/src/panel.rs` — `active`。

### Requirement: Minimal resume picker state
render_resume SHALL 绘制共享过滤分组结果并保存 hit areas，输出操作提示。

#### Scenario: 实现边界
- **WHEN** 搜索输入存在
- **THEN** 搜索 renderer 自行绘 cursor，本函数返回 None；提示不证明键盘 handler 已执行。

证据：`crates/codegen/pager-minimal/src/panel.rs` — `render_resume`。

### Requirement: Minimal MCP picker mappings
render_mcps SHALL 将共享服务器工具项映射到 data indices、group keys、labels，并 clamp selected；Loading/Error 清空映射。

#### Scenario: 实现边界
- **WHEN** 搜索过滤结果
- **THEN** subtitle 仍显示服务器总数，所有 non_selectable 标记为 false；键盘语义由宿主解释。

证据：`crates/codegen/pager-minimal/src/panel.rs` — `render_mcps`。

### Requirement: Minimal dropdown precedence
overlay active SHALL 按 file search、slash、completion 优先级取面板，渲染到 prompt 下方并加两行边框。

#### Scenario: 实现边界
- **WHEN** 高优先面板打开但结果为空
- **THEN** 返回无 dropdown，不继续选择低优先面板。

证据：`crates/codegen/pager-minimal/src/overlay.rs` — `active`。

### Requirement: Minimal viewport sizing
compute_target SHALL 优先 list、app modal/extensions、prompt modal，再普通内容；普通内容按2至ceiling限高而不强制 minimal_live_rows 底线。

#### Scenario: 实现边界
- **WHEN** 终端高度不足3
- **THEN** sync_viewport 不操作；预测有提交时只改 area 高，否则调用 set_viewport_height 并忽略错误。

证据：`crates/codegen/pager-minimal/src/overlay.rs` — `compute_target`。

### Requirement: Minimal prompt modal priority
active_modal SHALL 按 Cancel、Plan、Permission、Question、Rewind 选择替换 prompt 的模态。

#### Scenario: 实现边界
- **WHEN** 多个状态同时存在
- **THEN** 只绘优先项；这不执行批准、回答或取消动作。

证据：`crates/codegen/pager-minimal/src/overlay.rs` — `active_modal`。

### Requirement: Minimal question and permission editors
render_question SHALL 使用共享问题渲染和 scroll clamp，为自由输入保留前缀及多行 editor；cap 为屏高三分之一限制到3至15，再限剩余高。

#### Scenario: 实现边界
- **WHEN** permission 要求 inline followup
- **THEN** 按共享视图绘反馈 editor；实际问题与授权响应由宿主发送。

证据：`crates/codegen/pager-minimal/src/overlay.rs` — `render_question`。

### Requirement: Minimal live panel dispatch
draw_live SHALL 清旧 btw geometry，并优先绘 list、app modal、extensions，再 prompt modal 或普通输入布局。

#### Scenario: 实现边界
- **WHEN** 没有 agent 或区域太小
- **THEN** 无 agent 绘 startup，零高或宽小于4提前返回；有 agent 强制 Prompt active pane。

证据：`crates/codegen/pager-minimal/src/live.rs` — `draw_live`。

### Requirement: Minimal live content allocation
普通 live 布局 SHALL 先预留 status、prompt 和下方信息/dropdown，再给 btw、todo，剩余给 tail；btw 只在完整合法 rect 内绘并存 geometry。

#### Scenario: 实现边界
- **WHEN** 行为切换确认存在
- **THEN** 抑制 dropdown，显示行为提示并不返回输入 cursor。

证据：`crates/codegen/pager-minimal/src/live.rs` — `draw_live`。

### Requirement: Minimal visible tail clipping
draw_tail SHALL 从共享 frontier 测量全部余下条目，累计饱和 u16 高度，从顶部跳过溢出并绘最下方可见内容。

#### Scenario: 实现边界
- **WHEN** session reload 正在暂存
- **THEN** tail_height 为零且不绘 staged tail；并非只测可见末尾的虚拟列表。

证据：`crates/codegen/pager-minimal/src/live.rs` — `tail_height`。

### Requirement: Minimal status and input information
render_minimal_status SHALL 优先显示 transcript 进度，否则使用宿主 activity/control/watchers/queue 状态；不提供 mouse buttons，running execute 参数为 false。

#### Scenario: 实现边界
- **WHEN** 普通 prompt info
- **THEN** 显示模型reasoning、behavior、permission和非零上下文窗口；特殊模式可替换前述信息但保留queue/transcript提示。

证据：`crates/codegen/pager-minimal/src/live.rs` — `render_minimal_status`。

### Requirement: Minimal prompt info precedence
render_prompt_info SHALL 以 always approve、auto、ask 优先次序显示权限，并优先运行时 context total 再 model total，按顺序裁宽。

#### Scenario: 实现边界
- **WHEN** pending action 已过期或无 label
- **THEN** 不显示双击提醒；这些文字不实施权限或按键动作。

证据：`crates/codegen/pager-minimal/src/live.rs` — `render_prompt_info`。

### Requirement: Minimal transcript incremental build
pump_transcript SHALL 按 owning agent 的 ID 列表逐项构建，跳过已移除项，agent 不存在则丢弃 build，每项渲染后检查8ms预算。

#### Scenario: 实现边界
- **WHEN** 单条内容巨大或最终写文件
- **THEN** 可能超出8ms；ID 快照不冻结 entry 内容。

证据：`crates/codegen/pager-minimal/src/full_view.rs` — `pump_transcript`。

### Requirement: Minimal transcript expanded rendering
transcript 构建 SHALL clone entry 并设 Expanded，以100列输出已有内容；pump 临时启用 thinking 后正常恢复原可见性。

#### Scenario: 实现边界
- **WHEN** 历史摄入时已丢弃 reasoning 或渲染 panic
- **THEN** 不能恢复不存在的内容，也无 panic guard 保证 thread-local 恢复。

证据：`crates/codegen/pager-minimal/src/full_view.rs` — `render_entry_to_ansi`；`crates/codegen/pager-minimal/src/full_view.rs` — `pump_transcript`。

### Requirement: Minimal transcript file handoff
finish_transcript SHALL 将非空输出写 temp_dir 下 UUID ansi 文件并设置 pending pager；空结果或写失败向 owning agent 加 notice。

#### Scenario: 实现边界
- **WHEN** 等待外部分页器启动
- **THEN** 本函数不执行 pager 或删除文件，清理、权限与启动流程属于宿主。

证据：`crates/codegen/pager-minimal/src/full_view.rs` — `finish_transcript`。

### Requirement: Minimal ANSI serialization
buffer_to_ansi SHALL 去每行尾空格、跳过空 continuation symbol，样式变化输出 reset 和 SGR，行尾关闭链接并 reset 换行。

#### Scenario: 实现边界
- **WHEN** 样式包含 blink 或 hidden
- **THEN** 未编码这些 modifier；支持 bold/dim/italic/underline/reverse/strike 与 named/indexed/RGB/default颜色。

证据：`crates/codegen/pager-minimal/src/full_view.rs` — `buffer_to_ansi`。

### Requirement: Minimal transcript hyperlink routing
ANSI 链接 SHALL 使用 route 配置及首个覆盖 cell 的 span，打开 OSC8 时过滤 URL 控制字符，可附 ID，行尾关闭。

#### Scenario: 实现边界
- **WHEN** 普通 cell symbol 含控制字符
- **THEN** 本 serializer 不统一净化 symbol，依赖上游 renderer；不宣称完整终端净化。

证据：`crates/codegen/pager-minimal/src/full_view.rs` — `push_osc8_open`。

