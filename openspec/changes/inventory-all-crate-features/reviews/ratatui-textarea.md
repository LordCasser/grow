# ratatui-textarea 逐文件审阅记录

进行中：已全文阅读manifest、lib.rs、editor_keys.rs、editor.rs及editor_tests/{mod,editing,planning,viewport,keys}.rs。合并读取输出中planning尾部截断，已补读200至末尾闭合。textarea.rs仅读1–160行；render、wrapping、demo及textarea后续待读。不标reviewed、不运行动态测试。

## manifest与键盘分类

默认feature为空，debug-logs只启用可选tracing；生产依赖unicode segmentation/width、textwrap、两套ratatui接口及scrollbar。arboard等只dev，不能据此声称默认系统clipboard。

is_altgr仅Windows在去SHIFT后恰为CONTROL|ALT时true，其它平台false。classify_key_event不筛KeyEventKind，host需过滤release；裸C0 Ctrl-B/F只NONE，BS/DEL原字符忽略modifiers始终向后删grapheme。Ctrl+Alt+h精确组合优先Small word delete；Backspace只精确ALT或CONTROL各自匹配word，SUPER删行首，其余组合退回grapheme。这里Rust `ALT | CONTROL`是or pattern，不是组合值。Delete则任意包含ALT/CONTROL/SUPER即word，包括额外SHIFT。

Ctrl-W用WhitespaceDelimited；Alt/Ctrl方向small word，裸左右或Ctrl-b/f为grapheme，Ctrl-a/e逻辑行首末，Ctrl-u/k删至行界，Alt-b/f、Alt或Super-d等。普通NONE或SHIFT字符过滤control，SHIFT只补ASCII小写大写；AltGr路径在此前分派之后。Home/End、上下、Tab、Enter、undo、paste等留adapter，不从None推定产品不支持。

## EditBuffer完整实现

String字节光标，外部from_parts/set_cursor按最近grapheme边界、平局向左、越界clamp；from_text光标末尾。Clone复制值但新identity且generation0；Eq仅text/cursor。任何实际文字或光标变化递增generation，溢出换Arc identity归零；无变化不失效plan。

EditPlan保存range/replacement/removed/cursor/affinity与source identity/generation，字段私有且提供只读访问器。公开apply_plan先校验身份和代次，再range/UTF8/grapheme、removed匹配、结果长度和cursor；Exact只允许文本未变且原文grapheme边界，Right应用后向右靠齐新字素。正常内部apply/insert使用即时计划直接apply_validated。outcome区分未变/仅光标/仅文本/两者；delta的inserted_byte_range只标实际插入字节，不扩成合并后的完整字素。

atomic range先反转归序、clamp、外扩grapheme，去空，排序并仅合并重叠、不合并紧邻。非空replacement范围反复扩到所有交叠atomic边缘；零宽请求先nearest再atomic最近边（平局左）。导航和删除以atomic整段跨越，small word将每个atomic作为独立类别，WhitespaceDelimited把它并入Word。普通grapheme以首char分Whitespace/Word(alphanumeric或_)/Punctuation；不是自然语言分词。

word移动先吞前方/后方空白再消费同类项。逻辑行界只识别atomic外LF，CRLF末端回到CR前；重复行首/行末命令跨到前/下一行对应边界。DeleteToLineStart在行首删除前一个atomic/grapheme（含换行），DeleteToLineEnd在末端吞LF/CRLF；atomic中的换行不作为逻辑边界。

single_line_viewport宽0返回cursor空range；非零向左预算width-1再向右填，原子范围不可切开。此函数保留原cursor不做atomic归边；含atomic内部换行时可返回包含LF的所谓single line，测试明确此行为；超宽atomic在cursor前可能保留整段而超过width，不能扩展普通grapheme viewport宽度保证。没有按length设置资源硬预算，多次grapheme扫描属于现有算法。

## 内核测试证据

编辑测试覆盖ZWJ/combining/flag/CJK移动删除、Right affinity合并、范围替换前中后cursor、Small与WhitespaceDelimited、跨逻辑行及CRLF。planning覆盖identity/clone/stale/generation溢出、伪造非法range、atomic导航删除/word/隐藏换行；不据测试命名推导全部错误变体均测。

viewport测试覆盖窄宽/宽0、字素边界、CRLF和atomic换行/内部cursor，另固定seed2000次编辑与宽0..7检查普通非atomic不变量。keys测试覆盖精确modifier、raw encodings、host-owned键None、Windows条件分支；当前macOS未运行Windows路径。尚未动态运行任何本包测试。

## textarea.rs起始

ElementId私有u64但公开from_raw，ElementKind为host opaque u16。ClipboardProvider以可变get Option和无错误set接口；默认InternalClipboard仅内存clone。TextElement存public id/range/kind/可选display Line，声明原子语义需继续核对实际修改入口。选择anchor/head按字节；MouseAction区分cursor/selection/scroll，element事件Click/HoverEnter/Leave。ClickTracker从160行继续。


## textarea.rs推进至1180行

160–1180已逐行读取，鼠标Down主体从1181继续。

- ClickTracker同坐标且严格小于500ms时计1/2/3，达到3后下一次重置1。drag interval按80/60/40ms，distance实际0..2=1、3..5=2、6..10=3、其余5行，与注释区间不完全相同。clamp_to_line取最后char起点，不是直接grapheme起点，后续cursor归一化需继续跟踪。
- new默认内存clipboard、保留mouseup选择、show_scrollbar=true、padding0、tab_width4，以及固定RGB选区和scrollbar样式。set_tab_width只更新设置并清wrap cache，不重写既有文本。expand_tabs固定每tab若干空格，非tab stop对齐。
- semantic edit定义为文字变化或非空replaced range，因此相同文本非空替换也会更新elements、selection和cache，并可能创建undo步骤；EditOutcome可能仍Unchanged。公共try应用在pre_mutate前验证计划，内部非法计划panic。Kill覆盖kill_buffer为removed_text，不累积。导航清preferred_col与scroll_override。
- set_text先计划验证再Replace checkpoint，直接应用文字后清全部elements，光标保留旧位置并限新长、随后grapheme归一化。清selection/drag/hover事件/scroll状态但保留kill buffer；不是重置所有输入历史，undo实现仍待读。
- insert_str按首字符Whitespace与上一批末字符类别变化拆undo批；末字符写last_insert_ws。insert_str_at也记末类但不做相同首类拆批。空字符串两者直接返回；replace_range委托原子范围和tab规范化。
- set_cursor先clamp到element最近边界再EditBuffer边界。set_scroll_override注释声称外部设置不随cursor清除，但实际与内部共用字段，set_cursor及编辑/导航明确清None；规范须按实现，不能抄注释。
- desired_height直接wrapped_lines.len as u16，无极大文本checked转换。cursor_pos考虑scroll及内容宽，col>=tw时下一visual row起点，即最后一行恰好满也产生虚拟新行；screen_position_of不作此调整，任意pos验证需继续看width helpers。
- screen_spans_of_range拒空/越界/非char端点，按可见wrapped行交集产生一行高rect，裁content width，不包含无row归属LF/丢弃空白；并未要求grapheme端点。buffer_pos_at_screen检查完整area（含scrollbar列），超文本行回text.len；内部ex只拒左/上越界，不拒右/下，element_at_screen沿用ex，宿主边界不可由公共普通映射推断。
- element命中时使用pos在[start,end]查第一个元素，非hit则[start,end)，相邻元素共享边界归属需后续测试核对。selection_range对anchor/head排序、扩element再clamp长度；set_selection原样存字节且不验证UTF8边界，selected_text直接slice的安全性依赖扩边helper/调用者，待后续核对。
- delete_selection以Replace单步删除再光标回start并clear selection。clipboard notification与provider两路，只有非空复制才写；take_clipboard消费notification而非provider内容。element pending事件是单slot、poll take，不是无损事件队列。
- poll_timeout返回pending drag的整interval而非剩余时长；tick复用储存MouseEvent调用handle_mouse，需host周期调用，不自行起timer。element_click_snap单/双击命中原子元素时光标回start、设drag anchor并发Click。
- handle_mouse已读scrollbar前置分派：只用column判on_scrollbar无row区域检查；scrollbar_dragging下Down/Drag优先续拖、Up结束。鼠标Down若已有drag_active递归转Drag保anchor；正常Down先记click、清drag状态及现有selection，再映射位置（后段待读）。


## textarea.rs推进至2550行

1181–2550完整读取，完成handle_mouse、投影、input、undo以及部分垂直导航；Home后段从2551继续。

- Down先按旧scroll映射再清override。单/双击element都snap start发Click；普通双击按字符类别选词并复制，cursor取最后char再由EditBuffer归字素，空白仅放cursor；三击选择非element内部LF分隔的整逻辑行并包含尾LF，复制raw buffer。右/下越界ex的既有边界仍存在。
- Drag需要anchor，垂直出界保存event；首次因无last time立即滚动，后续按80/60/40节流（注释“first waits80”不精确）。回区域内仅清pending，不重置加速计数。按距离选target行、限制行尾最后char，保存override与raw selection head，然后cursor再归字素；选区端点与cursor可能不完全同一边界。Up清drag字段，仅was_drag时复制非空选区，keep_selection=false仅此路径清选择，零范围抖动不吞后续删除。
- Wheel不检查事件坐标，host必须路由；按高度1/2/3行改变override，拖动中selection head跟到目标行起点。max_scroll及total转u16，down普通加法/零高拖动target算式另有边界。hover元素改变先leave后enter但单slot覆盖，只保enter。
- scrollbar按相对row比例定位，height1=>0，越界row saturate后转换；thumb命中临时渲染CoreBuffer，当前offset只取override否则0，不用effective_scroll/state。渲染与命中一致性需后续测试验证。
- word_start/end按char而非grapheme扫描三类，并跳出elements。display_width_of_range在overlap从element start开始时计整个custom display宽，即只取部分raw也可计全宽；内部起点overlap退回raw宽。display_col_to_buffer_pos元素整单元按最近边（平局左），普通text按grapheme，partial element续行以raw宽并回实际end；依赖排序有效范围，不做所有公开pos入参校验。
- input不筛KeyEventKind。selection在Insert/Enter被undo group删除并替换，Backspace/Delete/raw/精确Ctrl-h/d删除选区，Ctrl-X复制删除；其它键先清selection，因此Ctrl-V/Ctrl-Y不是替换选区。通用分类优先后续adapter；Enter任意modifiers插LF，Ctrl-j/m精确、Ctrl-y kill yank，uppercaseZ含Ctrl/Super redo，lowercasez含Ctrl/Super undo（含额外modifiers）；Ctrl-V仅精确CONTROL读provider。Super左右精确，Up/Down/Home/End任意modifiers走导航，其它debug-logs可输出未处理event。
- undo snapshot仅text/cursor/elements，restore重建EditBuffer identity、清cache/preferred，但不还原next_element_id、kill buffer、selection/drag/hover或clipboard。默认最大100份全量snapshot。pre_mutate同Insert/Delete且cursor连续时合并，Kill/Element/Replace总拆步，变化前清redo；分组内完全跳过pre_mutate。
- clear_history清stack/redo与batch信息，未重置group_depth/checkpoint或last_insert_ws；因此活跃group中不能推导“完全硬重置”。undo/redo直接pop/restore，不禁止group期间调用；redo push未另做depth裁剪。
- group嵌套计数，outer end只比较text/cursor/elements.len，忽略相同元素数下的id/range/kind/display变化；cursor-only变化可建步。cancel无论嵌套退到outer并restore，但restore范围有限，scroll_override等不统一清。不是完整UI状态事务。
- delete n>1以group循环直到Unchanged，word/line kill取共享内核。kill_current_line使用adapter逻辑行边，非空删bol..eol，空行删前LF；CRLF实际行为待测试。yank独立kill buffer不读provider，不做insert_str首空白类拆批。
- move up/down有wrap cache时优先visual row及preferred display col，首末边跳text首末；无cache时raw find/rfind LF回退，不像其它logical helpers排除element内部LF，可能产生不同路径语义。读至Home bool分派，后续尚未审阅。


## textarea.rs推进至3510行

2551–3510完整读取，完成element API、wrap与主要渲染，truncate_line_display末尾从3511继续。

- Home/End无logical chain时优先cache visual边界；soft续行End取最后char位置再set_cursor归边，logical末段取exclusive end。
- insert/replace element经过plan与Element checkpoint，修改文字/旧elements后添加新range并cursor到end；空文本也可分配空element。add_element ID普通u64递增，无溢出轮换保障，sort仅按start。restore_elements直接加入元数据，不验证range合法/UTF8/重叠，也不undo checkpoint或cursor归边；set_element_display仅改display清cache。inline_element checkpoint后移除元数据、cursor end、清cache/preferred，last_kind None，但不post_mutate或清scroll_override。
- element_text直接slice登记range，非法restore范围可panic；只读elements返回slice。word_at_cursor按空白向两侧找，在词尾首空白位置可返回前词，和“cursor在whitespace总None”注释不一致，不考虑原子元素作为独立词。
- expand_range_to_element_boundaries只扩元素，不做UTF8/grapheme归一化，确认set_selection任意非char端点可流到selected_text直接slice；不要把编辑内核的范围归一化推导到所有查询。
- shift_elements去完全删除项，后项isize delta平移，部分overlap按新bounds退化；依赖有效非重叠元素，未统一验证所有元数据入口。
- wrapped_lines强制width>=1，单宽度RefCell缓存；仅存在任何custom display时走element-aware greedy，否则textwrap FirstFit，即无display元素仍可被普通wrap拆行。element-aware逻辑行跳过全部元素内LF（有无display都跳），空文本至少一空range。
- greedy元素整单元测display span宽或raw宽，超宽单元可独占超宽行，后续render截断。普通文本按grapheme，空格/tab和元素末尾可断点，换行丢新行首部分空格/tab；不以所有Unicode whitespace为断点。多次扫描重算无独立资源上限。
- effective_scroll先total as u16，内容fit强制0，然后override clamp，否则使cursor所属wrapped行可见；未处理cursor_pos特有的满行虚拟新行，不能从注释保证所有cursor情况可见。
- content_width以全宽wrap判断是否需要scrollbar，再扣1+padding，不作迭代稳定；宽<=1或show=false不扣，极大padding可扣成0而wrap内部仍按1。desired_height不等于带scrollbar最终高度。
- Stateless WidgetRef传全部lines给render_lines，不按area.height限制range；Stateful根据effective_scroll限行并写state.scroll。render_lines本身不纵向裁剪/清背景，直接buffer cell写入，宿主须传合法area并避免stateless长内容溢出。scrollbar由CoreBuffer绘后复制符号和自设style。
- render按plain/element/plain顺序累加display_x，plain按grapheme裁宽并展开tab；无display元素cyan，custom仅在首次overlap行绘，续行空且不占display_x。选区另算display列覆盖style，非整buffer清空。
- truncate_line_display读至span裁剪：按span宽累加，超宽预留ellipsis；尾] ) } >且max>=3保留闭括号。grapheme裁剪仅span内部，不能推导跨span完整字素；尾字符搜索跳空span但bracket style取最后span，即可能空span样式。函数收尾及测试待续。


## textarea.rs测试读至4650行

3511–4650全文读取。truncate结尾ellipsis继承最后保留span style，保留bracket继承先前选取style，返回新Line丢原Line层级属性；无截断clone保留。

已读测试覆盖adapter与内核8组命令一致、selection delta移动、相同文字替换移除metadata且undo恢复、element替换强制cursor end、空set_text清redo并建undo、零长element历史恢复、拒绝stale plan对text/elements/selection/kill/cache/undo无副作用。navigation边界也清preferred/scroll、元素内部插入归边、原子导航删除、忽略元素内LF的logical kill、ZWJ Right affinity undo/redo均有定向断言。

普通编辑测试覆盖insert位置与range前中后cursor、边界删除、Small word和line kill、Super方向/Backspace、Ctrl-U及原子forward delete。ID测试只验证小规模唯一和删除后稳定，不测u64耗尽。显示/metadata测试覆盖kind、element_at_cursor/text、display更新、宿主HashMap关联、排序；不能由这些正向测试认定restore_elements校验充分。

Buffer渲染检查custom display与cyan raw、后续文字按display宽定位，包括emoji多span。truncate bracket/宽0/宽1/普通ellipsis均有具体值；名为preserves_multi_span_styles测试主要断言宽度和末括号，未逐style验证，不扩写样式全覆盖。

tab测试检查默认4、自定义8、首尾/连续tabs、insert_at、set_text/replace、width0 passthrough、从0改4的存量tab显示宽、改tab_width不重写旧空格、多列粘贴、element插入/替换的expanded range。4650之后仍有tab及projection/undo/mouse等测试，未动态执行。


## textarea.rs测试推进至5300行

4651–5300完整读取，no_op_kill测试只读开头，后续待续。新增已读证据：Unicode与残留tab显示宽及cursor；tab helper borrowed/clip/paint；custom display投影与cursor（普通、emoji、CJK）、单span ZWJ裁剪、宽字符边界、括号省略；未验证跨span字素。

原子元素边界删除、左右跳过、word kill、line kill都有显式断言。若干名为ctrl_a/e测试实际直接调用bool=false导航且无wrap cache，是helper场景，不是input chord全流程或cache回退一致性证明。

wrap测试验证custom宽度影响行数、移到下行、无display普通文本适配、8列两行实际Buffer定位、custom隐藏内部LF以及后续cursor同visual行。部分断言仅len>=2，不能据其注释推导所有确切行分割。yank测试读完，验证最后kill内容恢复；空kill保持buffer的测试从5301继续。尚未运行动态测试。


## textarea.rs测试推进至5700行

5301–5700完整读取：no-op kill与set_text后kill_buffer保留均有定向验证；Ctrl-B/F及裸C0、Ctrl-W whitespace路径/标点/全空白/空输入、Alt/Ctrl Backspace Small语义、Ctrl/Alt Delete、窄不换行空格、Ctrl-Alt-H与Alt/Super-D有input级断言。raw BS/DEL额外modifier仍删单字素，DEL选区只删选区也有测试。

垂直移动测试检查preferred column、首末文本边及目标行范围；Ctrl-P/N部分只断言位于上/下方，不能扩大为精确列保持。此批无真实终端编码输入。home_end测试仅读开头，从5701继续；未运行动态测试，无新增构建缓存。


## textarea.rs测试推进至6130行

5701–6130完整读取，wrapped_navigation_with_newlines_and_spaces只读开头，从6131继续。

Home/End显式区分soft visual与logical chain，width4中间行End=7（最后char）、末行End=textlen；Alt箭头按hyphen三个chunk停点。cursor满行虚拟下一行测试使用height3有足够空间，不覆盖最后可见行已满的隐藏cursor疑点。

cursor/state测试覆盖fit忽略过大scroll、上/下越界自动跟随、Stateful绘后scroll及preferred列。screen_spans测试明确拒非char端点/空/越界，普通range跨行、offscreen头剔除、CJK宽、scrollbar内容边裁剪；不能扩展到未经同样验证的screen_position_of或selected_text。

wrapped navigation测试在预建cache后跨visual行并限短行末端，另逐次Stateful绘制验证滚动行坐标。部分测试禁scrollbar，部分依赖渲染重新按扣列宽建立cache；这些是已知宽度fixture，非任意缓存时序证明。未新增动态测试或构建产物。


## textarea.rs测试推进至6500行

6131–6500完整读取，screen mapping wrapped测试开头待读后段。补齐含空格/LF、宽emoji及ZWJ的visual导航，element-aware width2明确保留两个ZWJ完整范围。

fuzz_textarea_randomized实际seed是UTC减固定8小时日期，不是随夏令时变化的Pacific时区；每天变化，不是跨日期固定seed。500case×60step，宽1..12高1..4，18类编辑/导航/无display元素操作；不包括mouse、undo/redo、custom display、非法restore metadata或零尺寸。

随机测试只明确检查cursor<=len、仍可find到原payload时cursor不在内部、部分replace长度、两种render不panic及content fit时scroll0。payload“内容相同”assert来自find同字符串，不能发现已损坏而消失的payload；cursor坐标查询结果被忽略或None回退origin，没有断言x/y实际边界，与注释比证据更弱。Stateless特意分配完整desired高度，不能证明任意矮area安全。

鼠标映射已读plain首/中/行后/文本下方、offset、logical两行、左上越界测试；没有借这些公共API测试推导内部ex右下命中正确。未动态运行，无新增构建缓存。


## textarea.rs测试推进至6950行

6501–6950完整读取。screen mapping覆盖wrap、Stateful实际scroll、宽字素续cell回首字节、custom display最近边界、element hit/miss和空文本。selection测试仅正常ASCII范围排序、零范围、元素外扩与raw返回，未覆盖任意非char公开set_selection；默认选区逐cell前景背景有真实Buffer断言。

undo已读基础/redo新分支失效/光标还原/empty栈/Super-z及uppercaseZ/最大depth5/多轮roundtrip；多grapheme delete n=2单步恢复验证。深度测试通过私有max_depth=5和10次set_text，不代表快照字节内存有界。batch连续insert单步已读，后续batch/group与mouse测试从6951继续；未运行动态测试。


## textarea.rs 测试推进至8170行

6951–7370及7371–8170完整读取。多次删除跨原子元素的测试证明文本可单步撤销；另有元素撤销测试明确验证ID、范围、零长度元素与display文本恢复，display测试不验证样式。元素ID测试仅覆盖少量递增，不证明溢出安全。连续删除、插入调用间空白分类、类型变化和光标跳转分别影响合批；单次含内部空格的insert仍是一组。

分组测试覆盖autocomplete替换后插空格单步撤销、取消恢复外层快照且不新增undo、嵌套仅最外层提交、无变化无记录、提交清redo以及结束后正常合批。取消断言只覆盖文本、栈与group_depth，不能据此声称全部UI状态恢复。

鼠标测试覆盖点击定位、元素Click事件、行后/文本下方定位、左上外部拒绝、拖选正反方向、原子元素最近端点、松手复制与可选保留选区、零距离拖动、set_text清理拖动状态。越界测试是左上外部，不证明内部映射的右下边界。选区输入验证删除、输入替换单步undo、箭头清除、零宽选区继续处理按键；Ctrl-X零宽用例仅证明清除后无操作。

双击测试覆盖字母/下划线/标点分组、预组合é末字符定位、元素双击重新发Click且不复制隐藏文本；未据此推导组合字素选区安全。三击覆盖逻辑行及换行、末行至文本末尾和光标留在点击处。8170起custom selection style测试尚待下段补齐。

磁盘检查：本工作树target不存在，可用73GiB；本轮未启动构建。


## 库剩余测试、wrapping与示例

textarea.rs 8171–9759完整读取，库全部源码/测试读完。滚动测试验证wheel不移动光标、scroll后click按可见区映射、drag保留anchor/延长head、active drag期间重复Down继续拖动、距离档位1/2/3/5与间隔80/60/40ms。部分drag测试仅范围断言；名为drag_above_with_multibyte的用例area.y=0且row=0，实际没有越过上边界。计时继续滚动用例主动清私有timer，不能证明真实时间调度。

scrollbar测试覆盖溢出减宽1、关闭、不改变cursor、thumb不跳、track跳转与拖动；cursor_pos_accounts_for_scrollbar_width仅验证cursor=0返回origin，证据不能独立证明减宽边缘。样式用例部分仅断言有bg/非空symbol。Clipboard tests覆盖默认和自定义provider、Ctrl-X再Ctrl-V、拖选松手后provider回读。hover跨元素明确测试Leave被Enter覆盖。override测试覆盖连续render保持、max clamp和state+override保存恢复；不覆盖cursor移动后保持，不能支持注释中的持久override区别。

wrapping.rs、render/mod.rs、render/line_utils.rs完整读取。wrap_ranges只接受textwrap borrowed结果并通过指针范围回算byte range，Owned直接panic；普通版本额外保留紧随的ASCII spaces，trim版不追加。RtOptions默认OptimalFit高overflow penalty、HyphenSplitter、break_words=true、LF；indents扣宽后最低1，所以indent占满宽时结果仍可超宽。word_wrap_line先按initial宽获取首段，再按subsequent宽重wrap余下；跳过首段后ASCII spaces，后续empty范围跳过。源span样式再patch line.style；输出line源于indent，不能声称完整保留源alignment。owned/borrowed多行包装仅首个输入用initial indent。静态line clone保留alignment/style并owned内容；prefix_lines保留line.style但重建Line丢alignment；blank只认空或ASCII space，不认tab/newline。

examples/textarea_demo.rs完整读取（首次长输出中间截断，555–1405已分段补齐）。示例启动时ignore::Walk扫描cwd文件并排序，不刷新；Skim score降序最多8，空query取前8。@候选排除元素内部/前置alnum或下划线，token仅由whitespace、逗号、分号截止；确认整个token替换为文件元素并追加space且undo分组。系统剪贴板arboard按次创建，错误吞掉；多行paste为chip、单行plain，均为host逻辑。中键转交库原MouseEvent后paste，库只处理left Down，因此注释所称点击定位不成立。

行预览read_to_string全文件，空/不可读不进入；j/k、上下、Ctrl-u/d半页、f/b整页、g/G、数字累积立即goto。v循环Selecting/Locked，全选锁定/confirm去范围。parse range接受0与倒序，未验证；已有range取mid普通加法。live更新不断换element ID，host metadata追加未清旧值，也不随undo/cancel恢复。确认结束组、取消恢复textarea快照；从搜索进入读取失败仍提交已插元素。modal只拦key/mouse，Paste仍走全局paste。

示例渲染preview按char数截断而非cell宽，窄prompt不更新旧textarea_area；cursor在draw闭包设置，modal隐藏。main先启raw/alternate/mouse/paste/blink，再创建Terminal；仅run返回后best-effort清理，初始化中途错误/panic无RAII恢复。poll默认100ms或库拖动间隔，事件后也tick，仅有变化时重绘。示例Ctrl-Y拦成redo，与库kill yank不同。

已启动all-features测试，独立target、关闭incremental和debug信息；结果待确认，未标记crate完成。

验证结果：cargo test --locked -p ratatui-textarea --all-features（CARGO_INCREMENTAL=0、DEV/TEST_DEBUG=0，独立target）退出0，351 unit passed、0 failed、0 ignored；1 doctest ignored，没有通过的doctest。日志/tmp/grow-ratatui-textarea-tests.log。测试后立即cargo clean，移除1164文件349.7MiB（du分配量353M）。尚未完成feature-map/spec整合，保持pending。

## 规范登记

49项带场景与源码引用的要求已登记到textarea-editing-runtime，包含编辑内核、TextArea状态/渲染、styled wrapping与示例宿主能力。全部13份Rust文件及Cargo.toml纳入证据哈希。
