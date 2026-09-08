## ADDED Requirements

### Requirement: Textarea grapheme cursor normalization
EditBuffer SHALL 保存UTF-8文本和byte cursor，外部位置clamp并选择最近extended grapheme边界，距离相同取左。

#### Scenario: 当前实现
- **WHEN** 从文本构造或设置越界位置
- **THEN** from_text光标在末尾；from_parts及set_cursor_byte规范化，读取/into_text不额外变换文本。

证据：`crates/codegen/ratatui-textarea/src/editor.rs` — `normalize_external_cursor`。

### Requirement: Textarea plan identity and generation
EditBuffer SHALL 将计划绑定实例身份和generation，Clone使用新身份，文本或光标实际变化推进generation。

#### Scenario: 当前实现
- **WHEN** 应用旧计划或克隆前计划
- **THEN** 验证拒绝；无变化计划可重复用，generation溢出换新身份；Eq仅比较文本和光标。

证据：`crates/codegen/ratatui-textarea/src/editor.rs` — `advance_generation`。

### Requirement: Textarea edit plan validation
apply_plan SHALL 先验证身份、generation、范围、被替换文本、结果长度和光标约束，再修改。

#### Scenario: 当前实现
- **WHEN** 计划失效
- **THEN** 返回ApplyEditPlanError且不应用；内部即时计划可走已验证路径，不提供跨实例通用patch。

证据：`crates/codegen/ratatui-textarea/src/editor.rs` — `validate_plan`。

### Requirement: Textarea edit delta and affinity
编辑 SHALL 分别报告未变、cursor变、text变及两者变，delta保存替换范围和插入字节。

#### Scenario: 当前实现
- **WHEN** 插入与邻接字符形成新字素
- **THEN** Right affinity向右取新字素边界；delta不是合并后完整字素内容，Exact只适用于允许的原边界条件。

证据：`crates/codegen/ratatui-textarea/src/editor.rs` — `apply_validated_plan`。

### Requirement: Textarea atomic range normalization
计划 SHALL clamp、排序并按字素外扩atomic ranges，剔除空项且合并重叠项。

#### Scenario: 当前实现
- **WHEN** 两个range仅相邻
- **THEN** 不合并；非空替换扩到所有重叠原子边界，零宽插入选最近边界。

证据：`crates/codegen/ratatui-textarea/src/editor.rs` — `normalize_atomic_ranges`。

### Requirement: Textarea word and line commands
plan_command SHALL 支持字素移动/删除、Small和WhitespaceDelimited词移动/删除、逻辑行首尾及kill。

#### Scenario: 当前实现
- **WHEN** 遇原子元素和CRLF
- **THEN** Small将元素作为独立类，WhitespaceDelimited当Word；原子内LF不分行，CRLF行尾在CR前，重复行首尾可跨换行。

证据：`crates/codegen/ratatui-textarea/src/editor.rs` — `plan_command`。

### Requirement: Textarea single line viewport
single_line_viewport_with_atomic_ranges SHALL 按cursor左侧width减1预算再扩右侧，返回可见byte range及cursor显示位置。

#### Scenario: 当前实现
- **WHEN** width为0或元素超宽
- **THEN** 零宽返回空；原子元素可超过预算，原始cursor在原子内部不被该投影改写，不承诺严格cell上界。

证据：`crates/codegen/ratatui-textarea/src/editor.rs` — `single_line_viewport_with_atomic_ranges`。

### Requirement: Textarea key classifier modifier rules
classify_key_event SHALL 识别普通/Shift字符、Emacs控制键、词与字素导航删除，并过滤未支持的控制字符。

#### Scenario: 当前实现
- **WHEN** SHIFT数字或标点
- **THEN** 信任终端给出的字符，仅ASCII字母转大写；原始BS/DEL控制字符走后删，不按额外modifier重新解释。

证据：`crates/codegen/ratatui-textarea/src/editor_keys.rs` — `classify_key_event`。

### Requirement: Textarea AltGr platform boundary
is_altgr SHALL 仅Windows上在去掉SHIFT后精确匹配CONTROL加ALT。

#### Scenario: 当前实现
- **WHEN** 其他平台相同组合
- **THEN** 不插入AltGr字符；Ctrl-Alt-h删除词先于AltGr。KeyEventKind不参与过滤，宿主须按需过滤release/repeat。

证据：`crates/codegen/ratatui-textarea/src/editor_keys.rs` — `is_altgr`。

### Requirement: Textarea default state and tab expansion
TextArea new SHALL 使用内部clipboard、保留松手选区、启用scrollbar、padding0、tab宽4及默认样式。

#### Scenario: 当前实现
- **WHEN** 改变tab宽
- **THEN** 清wrap cache；按固定空格数扩tab，不采用tab stops，也不改写既有文本。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `set_tab_width`。

### Requirement: Textarea semantic replacement metadata
TextArea SHALL 将非空替换范围视为semantic edit，即使新旧字节相同也更新元素/选区/缓存与undo。

#### Scenario: 当前实现
- **WHEN** 底层EditOutcome为Unchanged
- **THEN** 不因此跳过metadata维护；失效计划必须先拒绝，不能产生局部UI修改。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `is_semantic_edit`。

### Requirement: Textarea set text reset scope
set_text SHALL 建立替换checkpoint、清元素和选区/拖动/hover/scroll override，并clamp旧cursor。

#### Scenario: 当前实现
- **WHEN** 重置内容
- **THEN** 保留kill buffer和undo历史；不是清除整个TextArea状态。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `set_text`。

### Requirement: Textarea insertion batching
insert_str SHALL 在调用间首字符空白类别变化时断开insert batch，保存尾字符类别。

#### Scenario: 当前实现
- **WHEN** 一次插入包含内部空格
- **THEN** 仍为一次插入；insert_str_at没有相同首字符分批逻辑，空插入不修改。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `insert_str`。

### Requirement: Textarea selection API boundary
selection_range SHALL 排序anchor/head、clamp并向元素边界扩展，零宽返回None。

#### Scenario: 当前实现
- **WHEN** 调用方set_selection给非UTF-8边界
- **THEN** setter不验证，selected_text直接slice可能panic；不能将cursor字素保证推广到任意选区输入。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `set_selection`。

### Requirement: Textarea selection keyboard replacement
input SHALL 对有选区的字符/Enter执行分组替换，对删除键执行仅删除选区，Ctrl-X复制后删除。

#### Scenario: 当前实现
- **WHEN** 按Ctrl-V或Ctrl-Y
- **THEN** 先清选区再执行普通粘贴或yank，不替换所选文本；零宽选区清除后继续处理按键。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `input`。

### Requirement: Textarea host input extensions
TextArea input SHALL 在共享分类器之外支持Enter换行、Ctrl-j/m、Tab、Up/Down/Home/End、undo/redo和clipboard。

#### Scenario: 当前实现
- **WHEN** Ctrl-Y与Ctrl-V
- **THEN** 前者只yank kill buffer，后者读取provider；大小写Z及modifier规则按实际match，示例可另行拦截。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `input`。

### Requirement: Textarea clipboard provider and notification
ClipboardProvider SHALL 提供get Option及无返回值set，TextArea复制非空文本时调用provider并保存独立通知。

#### Scenario: 当前实现
- **WHEN** 调用take_clipboard
- **THEN** 消费通知而不清provider；库默认内存clipboard，系统clipboard由宿主提供。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `set_clipboard_text`。

### Requirement: Textarea undo snapshots and capacity
undo SHALL 保存文本、cursor和元素完整快照，默认最多100个undo checkpoint。

#### Scenario: 当前实现
- **WHEN** undo或redo恢复
- **THEN** 不恢复选区、拖动、hover、clipboard、kill、next ID；快照数限制不是字节内存限额。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `snapshot`。

### Requirement: Textarea undo batch boundaries
undo合批 SHALL 受编辑类型、连续cursor和insert空白类别约束，Kill/Element/Replace为离散操作。

#### Scenario: 当前实现
- **WHEN** 调用clear_history
- **THEN** 不重置所有group/checkpoint/空白分类状态；undo后重置普通合批，redo新分支被新编辑清除。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `clear_history`。

### Requirement: Textarea nested undo groups
begin_undo_group SHALL 只在最外层保存checkpoint，最外层end产生一次记录，cancel恢复最外层并结束嵌套。

#### Scenario: 当前实现
- **WHEN** 组内仅同数量元素metadata变化
- **THEN** 提交比较text、cursor和elements.len，可能不记录；cursor独变可记录，组内undo/redo没有禁止guard。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `end_undo_group`。

### Requirement: Textarea kill overwrite and multi delete
kill操作 SHALL 覆盖kill buffer而非累积，yank插入该buffer，多次字素删除以组支持单步撤销。

#### Scenario: 当前实现
- **WHEN** kill无内容或set_text
- **THEN** 不清既有kill内容；adapter当前行kill与底层逻辑行算法须分别按实现判断CRLF。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `yank`。

### Requirement: Textarea vertical navigation cache
上下移动 SHALL 在有wrap cache时使用visual rows与preferred column。

#### Scenario: 当前实现
- **WHEN** 尚无cache
- **THEN** 使用原始LF查找，可能将原子内LF作为换行；Home/End false有cache走visual，无cache退化。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `move_up`。

### Requirement: Textarea element creation and identity
insert_element及replace_range_with_element SHALL 通过编辑计划创建带kind、range和可选display的原子元素，包括空元素。

#### Scenario: 当前实现
- **WHEN** 撤销后再次插入
- **THEN** ID不随undo倒退；u64普通递增无显式溢出guard，调用方metadata不自动随快照更新。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `insert_element`。

### Requirement: Textarea element restoration and display mutation
restore_elements SHALL 接受宿主metadata，set display清缓存，element查询提供raw backing text。

#### Scenario: 当前实现
- **WHEN** 宿主提交非法range或重复ID
- **THEN** 无统一校验，也不自动创建undo checkpoint；不能假定所有公开入口维持合法metadata。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `restore_elements`。

### Requirement: Textarea inline element conversion
inline_element SHALL 保留raw文本、移除元素metadata、cursor移到元素尾并建立undo checkpoint。

#### Scenario: 当前实现
- **WHEN** ID不存在
- **THEN** 返回false；成功路径不等同普通post-edit，不自动恢复或更新宿主metadata。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `inline_element`。

### Requirement: Textarea element shifts after edits
元素维护 SHALL 删除被完全覆盖项、平移后续项，将部分重叠项退化为普通文本。

#### Scenario: 当前实现
- **WHEN** 编辑命中已有元素
- **THEN** 按规范化计划更新；恢复入口仍依赖宿主range有效性。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `shift_elements`。

### Requirement: Textarea wrapped display projection
显示投影 SHALL 将custom display宽度代替元素raw宽度，屏幕列选择最近元素端点，普通字符按字素计算。

#### Scenario: 当前实现
- **WHEN** 部分range开始于元素内部
- **THEN** 不能把该range当完整display重复计数；命中边界及跨行继续段按当前实现处理。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `display_col_to_buffer_pos`。

### Requirement: Textarea cursor and screen range mapping
cursor_pos_with_state SHALL 考虑content width和scroll，满行末cursor可投影到下一虚拟行。

#### Scenario: 当前实现
- **WHEN** 调用screen_spans_of_range
- **THEN** 拒绝非法char范围并裁剪可见交集；screen_position_of不采用同样满行调整，不能视为相同API。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `screen_spans_of_range`。

### Requirement: Textarea mouse mapping boundary
buffer_pos_at_screen SHALL 对公开area做完整坐标边界检查，行后定位行尾、文本下方定位末尾。

#### Scenario: 当前实现
- **WHEN** element_at_screen经内部ex映射
- **THEN** ex只检查左上界，右下和scrollbar范围不能据公开方法推导安全命中。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `buffer_pos_at_screen_ex`。

### Requirement: Textarea click and multi click
同坐标500ms内click SHALL 循环单/双/三击，位置变化重置；单击定位，双击词，三击逻辑行含尾LF。

#### Scenario: 当前实现
- **WHEN** 点击custom元素
- **THEN** 单/双击发Click并定位元素首，不复制hidden backing；三击仍按逻辑行，双击词分类按char而非完整字素。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `handle_mouse`。

### Requirement: Textarea drag selection lifecycle
左键drag SHALL 以anchor扩选，松手复制并按配置保留选区，零距离拖动不留下选区。

#### Scenario: 当前实现
- **WHEN** active drag期间终端重发Down
- **THEN** 继续拖动不重置anchor；拖出垂直边界延伸选区，cursor规范化不等于selection head字素规范化。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `handle_mouse`。

### Requirement: Textarea wheel viewport scrolling
wheel SHALL 使用scroll override移动viewport，普通滚动不移动cursor；拖动中保留anchor并调整head。

#### Scenario: 当前实现
- **WHEN** 内容完全fit
- **THEN** 返回Nothing；wheel路径不检查事件是否在area内，需要宿主路由。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `handle_mouse`。

### Requirement: Textarea drag timer integration
poll_timeout_ms和tick SHALL 暴露拖出区域后的连续滚动需求，宿主驱动timer，间隔80/60/40ms递进。

#### Scenario: 当前实现
- **WHEN** 鼠标距边界增大
- **THEN** 距离档位0..2为1行、3..5为2、6..10为3、更远5；首次可立即滚动，timeout是完整间隔而非剩余时间。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `poll_timeout_ms`。

### Requirement: Textarea scrollbar interaction
scrollbar SHALL 按溢出显示并占1列加padding，track click按比例跳转、thumb click启动无跳转drag。

#### Scenario: 当前实现
- **WHEN** 点击scrollbar列但row越界
- **THEN** 路径不做完整row界限；thumb以override或0计算，不保证与cursor-follow state始终一致。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `is_scrollbar_thumb_at`。

### Requirement: Textarea element event single slot
poll_element_event SHALL 消费最新单个Click/HoverEnter/HoverLeave，停留同元素不重复enter。

#### Scenario: 当前实现
- **WHEN** 直接从元素A移到B
- **THEN** A的Leave会被B的Enter覆盖，不是无损事件队列。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `poll_element_event`。

### Requirement: Textarea scroll override lifetime
set_scroll_override SHALL 设置与wheel共享的override，render clamp至max并优先于cursor-follow。

#### Scenario: 当前实现
- **WHEN** 后续cursor移动或编辑
- **THEN** 公共override也可能被清除；只跨render保持，不能声称独立持久模式。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `set_scroll_override`。

### Requirement: Textarea plain versus element aware wrapping
wrapped_lines SHALL 缓存一个width，最低width1；存在任何Some display时才选择element-aware wrap。

#### Scenario: 当前实现
- **WHEN** 全部元素无display
- **THEN** 走普通textwrap FirstFit，可能在raw原子内部wrap；element-aware对原子内LF不分行，超宽元素独占行但可超宽。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `wrapped_lines`。

### Requirement: Textarea viewport width and height accounting
content_width SHALL 先按全宽判断overflow，再扣scrollbar宽，desired_height返回wrapped count转u16。

#### Scenario: 当前实现
- **WHEN** 极多行或极窄area
- **THEN** 不是全面checked几何；不反复求固定点，desired_height不等同带最终scrollbar宽的渲染高度。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `content_width`。

### Requirement: Textarea stateful and stateless rendering
StatefulWidgetRef SHALL 只渲染可见rows并更新scroll，Stateless渲染全部wrapped rows。

#### Scenario: 当前实现
- **WHEN** 宿主给Stateless矮buffer
- **THEN** render_lines没有统一垂直裁剪/背景清理，可能越界；需匹配尺寸，不能由Stateful安全推导Stateless。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `render_lines`。

### Requirement: Textarea element and selection styling
render_lines SHALL 对custom display绘制替代内容，无display元素用元素样式，再覆盖选区style。

#### Scenario: 当前实现
- **WHEN** 元素跨wrapped rows
- **THEN** continuation显示和宽度按当前投影处理；不把raw payload自动当普通逐行文字。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `render_lines`。

### Requirement: Textarea styled truncation
truncate_line SHALL 以span内grapheme和cell宽截断并追加省略号，宽度允许时保留尾部闭括号。

#### Scenario: 当前实现
- **WHEN** 多个span或Line属性
- **THEN** 不跨span重新组合字素；截断重建Line会丢部分Line属性，未截断clone保留，样式按实际保留片段选择。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `truncate_line_display`。

### Requirement: Textarea wrapping options
RtOptions SHALL 支持宽度、LF/CRLF、首行/后续indent、break_words、算法、分词与分割器。

#### Scenario: 当前实现
- **WHEN** indent占满width或break_words=false
- **THEN** 内容预算仍最低1，输出可以超宽；默认OptimalFit高溢出惩罚不是绝对禁止溢出。

证据：`crates/codegen/ratatui-textarea/src/wrapping.rs` — `RtOptions`。

### Requirement: Textarea styled word wrap range reconstruction
word_wrap_line SHALL flatten spans后用textwrap range切回styled内容，首段和余段分别按indent预算wrap。

#### Scenario: 当前实现
- **WHEN** textwrap返回Owned
- **THEN** range helper panic；borrowed指针必须落在输入内，余段空range跳过，只专门跳ASCII spaces。

证据：`crates/codegen/ratatui-textarea/src/wrapping.rs` — `word_wrap_line`。

### Requirement: Textarea line ownership and prefixes
line_to_static SHALL 复制span内容并保留line style/alignment，prefix_lines按首/后续prefix重建。

#### Scenario: 当前实现
- **WHEN** prefix重建line
- **THEN** 保留style但不保留alignment；blank helper只认空或ASCII spaces，不认tab与newline。

证据：`crates/codegen/ratatui-textarea/src/render/line_utils.rs` — `prefix_lines`。

### Requirement: Textarea randomized test scope
随机测试 SHALL 运行500乘60操作，width1..12、height1..4及固定UTC减8日期seed。

#### Scenario: 当前实现
- **WHEN** 测试通过
- **THEN** 不证明mouse、undo、custom display或非法metadata覆盖；payload find后自比较不能发现消失payload，cursor坐标未完整断言。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `fuzz_textarea_randomized`。

### Requirement: Textarea example file completion
textarea_demo SHALL 启动时扫描cwd文件并按Skim分数显示最多8项，@上下文确认整token成元素及尾space。

#### Scenario: 当前实现
- **WHEN** 文件列表变化
- **THEN** 不自动刷新；这是示例host能力，库不内建文件搜索。

证据：`crates/codegen/ratatui-textarea/examples/textarea_demo.rs` — `FileSearch`。

### Requirement: Textarea example file line selection
示例 SHALL 读完整UTF-8文件，提供行移动/分页/goto、v选择锁定、live更新文件引用以及confirm/cancel undo组。

#### Scenario: 当前实现
- **WHEN** 空/不可读文件或非法range文本
- **THEN** 无法进入预览；parse允许0/倒序，metadata随新ID追加且不随undo恢复，不作为生产文件引用契约。

证据：`crates/codegen/ratatui-textarea/examples/textarea_demo.rs` — `LineSelectMode`。

### Requirement: Textarea example clipboard and terminal lifecycle
示例 SHALL 通过arboard接入系统clipboard，单行paste plain、多行chip，并在事件循环调用tick。

#### Scenario: 当前实现
- **WHEN** 中键粘贴或初始化失败
- **THEN** 中键转交原事件不实际定位cursor；仅run返回后清理terminal，初始化失败及panic不具RAII恢复。

证据：`crates/codegen/ratatui-textarea/examples/textarea_demo.rs` — `main`。

### Requirement: Textarea undo input detection
is_undo_input SHALL 只识别小写z且含CONTROL或SUPER，不检查KeyEventKind。

#### Scenario: 当前实现
- **WHEN** 输入大写Z或额外SHIFT
- **THEN** 大写Z不被helper视为undo；小写z加SHIFT仍被识别，不能把helper当完整redo分类器。

证据：`crates/codegen/ratatui-textarea/src/textarea.rs` — `is_undo_input`。

