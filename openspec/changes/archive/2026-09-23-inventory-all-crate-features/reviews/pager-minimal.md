# pager-minimal 逐文件审阅记录

最终状态：12个Rust文件及Cargo.toml已全部阅读，36项功能已映射至minimal-terminal；以下为分段阅读日志，阶段性待办以后面的闭合记录为准。

- manifest没有自定义feature，直接依赖pager读取view model，通过pager的minimal_hook函数指针接缝反向注册draw以避免Cargo循环；ratatui-inline用于重导出terminal/doc链接，dev pager启用test-support。
- draw顺序：queue同步更新起始（错误忽略）、autoresize（错误忽略）、sync_pending_marks、pump_transcript、welcome、plan、prepare_live_tail_display、sync_viewport、commit_active；若返回写入失败标志重新sync_viewport，然后expand_pending和draw_live。匹配End由后续draw_frame路径承担，尚需读live验证，不从注释直接断言所有错误路径都闭合。
- install只提交draw hook；重复调用忽略属于pager接缝行为待宿主核对。终端原生scrollback保存已提交内容，用户历史交互语义由后续commit/full_view读取确认。
- guard测试对列出的10个生产模块文本禁止resize_purge_rerender、emit_to_scrollback、resize_viewport_height字符串；是运行测试时的字符串扫描，不是静态类型约束或完整调用图证明。没有扫描独立commit_tests或未来新增模块，维护需同步列表。
- todo：空集合隐藏；force显示非空列表，否则至少一项Pending/InProgress才显示，与agent当前运行态无关。默认高最多8行，force按总长度as u16返回，由调用方再限屏幕。实际lines函数不自动调用visibility，max_rows为0直接空；溢出保留最后一行显示剩余数，force时去ctrl+t提示。显示原顺序第一行trim后最多64个Unicode字符（不是终端cell或grapheme），pending方框、进行中三角加粗warning、完成check、取消划线。render设置Reset背景并按area宽高裁剪。测试覆盖全部完成隐藏、强制显示、零行、状态glyph、溢出计数和ASCII省略，不证明复杂字素裁剪安全。
- panel前段：Resume优先MCP tab，仅这两个变下置列表，其它modal继续原路径；高度是4行chrome加测量body，最低5，即ceiling<5也会返回5。render只防height<2或width<8，而chrome布局按4行计算，窄高边界须继续核对调用方。Resume使用共享filtered/grouped builder、搜索栏和picker renderer，保存hit areas；函数最终返回None，搜索cursor由搜索栏绘制而非返回hardware cursor。MCP后段尚待补读。

## panel 补读完成、plan 全文完成

panel.rs 330–末尾已按区间补读，连同此前前段完成全文。MCP共享builder输出labels/group keys/data indices；section折叠状态考虑searching，server行indent1、工具子行indent2，disabled覆盖status badge、右侧tool数量。Loading/Error清空所有行并显示subtitle，Loaded subtitle是服务器总数而非过滤数。渲染前覆盖entry_data_indices/group_keys/labels_cache，两种non_selectable数组全false，selected越界clamp，empty重置0；hit areas没有search/tab/filter/close区域。键盘动作由pager handler解释，这里仅显示提示及更新映射。measure_entries为header首1其余2，展开row计description/fields，折叠计summary；使用saturating累加但len as u16可先截断。

panel测试覆盖active选择、MCP正常渲染+映射长度、Resume提示、14列emoji/组合字素搜索栏与共享renderer逐cell一致，以及5行高度限额。未见实际按键驱动、错误/加载状态或2–4行绘制测试；不能由footer文本证明键盘行为正确。

plan.rs全部阅读：仅active Agent且有approval view时取tool_call_id/content，对minimal_committed_plan_id去重；构造agent_message，存在pending tool anchor时插其前，否则追加。只有push后设置已加入ID，此标记表示模型块入scrollback，不等于终端写成功。已完成计划的重放耐久性声明仍需shell/pager验证，不扩成持久化契约。

controls高度Preview/Commenting=2，Prompt=3；无focus默认Preview。render仅拒height0或width<4，控制行按底部定位；是否有comments或非空prompt决定Prompt下enter提示approve/request changes，Commenting提示save，Preview提示a/s/q。仅Prompt绘制单行无chrome反馈输入并返回cursor。窄高区域的重叠或越界防护依赖调用布局，未在此完整保障。plan文件无内嵌单测，相关commit_tests尚待读。

## startup/welcome 全文与 full_view 起始

startup.rs与welcome.rs全文完成；full_view.rs读至280行，cell_sgr及测试待续。

- startup按TrustState Pending显示路径信任提示，Done显示Starting；只呈现文案，不执行信任决定。路径过滤控制字符，按Unicode scalar计列和逐cell set_char，不是宽字符/grapheme宽度算法；提示固定行本身不换行，仅路径换行。零区域直接返回，路径长度计数as u16有极端截断边界。文件无内嵌测试。
- welcome pending且宽>=8才尝试，把viewport移到0,0并clear（错误忽略），打印版本、可选cwd/model和help提示，圆角边框，高度info行数+4。insert_before失败保留pending，下帧重试；成功才清标记并insert_gap。重试仍会clear可见区，不从“pending未清”推导无部分终端写入。测试仅卡片高度三个例子，未验证terminal失败/重试或真实scrollback保留。标记由宿主设置，非本包自动按session ID去重。
- full_view：每帧take build，用其owning agent而非active view解析EntryId；agent移除则丢build，已移除entry跳过。每个entry完整渲染后才检查8ms预算，因此单条大entry可超时，最终整文件write也非切片。引用ID快照，不是内容不可变快照；期间更新可能反映在后续条目。
- 临时覆盖thread-local thinking可见性并正常路径恢复，非panic guard；仅显示模型中已有thinking，不能恢复摄入时已丢弃的内容。entry clone设Expanded，固定100列，flat背景，renderer决定高度，route决定OSC8及ID。
- 完成后空输出给owning agent notice；非空用temp_dir/UUID.ansi直接fs::write并设置pending pager true，写失败notice。调用外部分页器、文件清理/权限属于宿主待核对，本文件未在已读段设置独占权限或unlink。
- ANSI逐行裁掉尾部空/单空格cell，空symbol视wide continuation跳过，按fg/bg/modifier变化输出SGR，行尾reset和newline；每位置线性找首个link span，变更时关闭/打开OSC8，URL去控制字符，行尾关链接。单元symbol内容直接输出；是否包含控制字符由上游renderer保证，不能在此宣称普遍净化。

## full_view 全文完成；overlay 读至245行

full_view补读281–末尾完成。SGR支持bold/dim/italic/underline/reverse/strike，未编码其它modifier如blink/hidden；每次style变化先reset，支持named/indexed/RGB及default颜色。测试覆盖thinking显示开关机制、流式thinking正文、collapsed展开、cwd相对路径、颜色/SGR、尾空格清理、24列CJK链接label和完整OSC8目标。thinking测试直接切thread-local并最终设false，没有执行pump或验证原值恢复；未验证跨agent切换的完整build生命周期、8ms预算、临时文件失败或pager启动。全保真限定为已有entry及renderer/serializer支持的样式，不宣称所有终端属性都保留。

overlay前段：dropdown优先file search、slash、completion；高优先打开但零结果返回None并抑制低优先。file/completion数量先as u16再cap，slash按共享wrapped rows测量；面板额外两border行。sync_viewport终端height<3直接返回；target不变不操作，will_commit时仅改viewport area高度，非commit时set_viewport_height并忽略错误。will_commit拒绝无active agent、app modal和session reload，采用共享scan_frontier预测，具体扫描待commit审阅。compute_target开头ceiling=max(term_h-1,3)，base clamp3..ceiling，使用关闭timestamps的committed appearance测tail；非agent启动提示分支从246行继续。

## overlay 生产实现全文完成

补读246–880行，生产代码全部读完，测试从question editor起读至其首个高度断言，880之后待续。

- target优先list panel，再app modal或extensions，再prompt-replacing modal，最后普通tail/todo/btw/status/prompt/dropdown。普通target按内容clamp2..ceiling，不以minimal_live_rows为底线；后者base主要用于启动/模态。行为切换hint存在时不预留dropdown；无dropdown至少保留下方info行。btw先扣chrome算可见高，与shared minimal policy一致。
- dropdown实际渲染给shared chrome的minimal=true，明确位于输入框下方，与文件头/部分doc“above”描述不一致；file search先ensure_visible，slash/completion沿用共享renderer。
- prompt替换模态优先Cancel、Plan、Permission、Question、Rewind；permission取队首，question高度加入多行editor额外行，full screen高度用于shared cap。modal target保留tail+状态，超屏裁剪由live承担；app modal目标18行clamp base..ceiling，非近全屏旧注释。
- render_app_modal仅dispatch共享renderer。Cancel临时收集按钮hit rect后写回；Permission共享视图返回inline_prompt才画反馈editor，剩余高度至少1。Question按输入高度扣主区，按当前可见options clamp_scroll，然后共享渲染；freeform附z、单选/多选marker和prompt arrow，编辑器x+11，用shared inline_text_width。cap=floor(screen_h/3) clamp3..15，实际绘制再限modal剩余行。只有SafeBuf marker写入明确裁界，prompt矩形边界仍依赖共享renderer。
- 各路径绘制/测量不实施批准或回答动作，实际键盘handler留在pager包；布局状态变化不能当用户授权。

## overlay 全文阅读闭合

补读881–末尾，完成测试阅读。question多行输入测试实际绘制Buffer并断言cursor、前缀和三行正文；30行输入测试检查预留cap13、问题标题仍可见、editor定位。其余主要直接验证completion状态、加边框行数、content/modal高度公式及btw宽11/12、高2/3边界，不运行真实terminal grow/shrink或键盘事件。尚未动态运行这些测试。

协作消息报告main新增fix-disabled-skill-preloading等技能修复，未合入当前分支，不计当前源码证据；后续集成需重新核对。

## live 读取至595行

- draw_live每帧先清btw geometry，再通过共享draw_frame绘制。height0/width<4早退；无agent呈现startup。有agent时ensure media paths、强制Prompt active pane并解析activity；list panel、app modal、extensions依次优先，均早退跳过普通布局。
- prompt替换模态保留状态行和tail，reload时不画tail。普通布局优先status/下面dropdown或info/prompt，再btw、todo，余高给tail。btw仅在整个rect位于buffer且shared几何允许时绘制并保存geometry，避免隐藏面板保留滚动区域。行为确认时禁dropdown、信息栏先显示behavior hint，其次pending action，再普通info；prompt仍画但返回cursor=None。
- prompt_style始终focused，采用模式accent/prefix/placeholder，背景Reset，无border/accent line/左右pad，image_preview启用；共享appearance控制prefix/compact。
- draw_tail从scan_frontier.tail_start收集全部剩余entries，每项测height并加gap（包括0高项），总高u16 saturating，超出区域从上方跳过，with_skip_rows裁首可见项；不是只测可见末尾，巨量history开销仍需关注。OSC8取与skip renderer一致的链接span，route开关决定发出。
- 状态条transcript进度优先正常activity；无control_status且shared should_show=false才idle hint。control/watchers/drain/parked与计时均读取pager状态；minimal_advance_phase_timer实际只resolve，注释说明转换由reducer承担。595行之后参数、info、tail_height及测试待续。

## live 生产路径完成，测试读至925行

状态条传递activity/turn计时、MCP初始化、watchers/parked/held queue/control，禁mouse buttons，has_running_execute固定false。idle文字取宿主auto-set和switch-back标志，不能仅由文案断言平台自动切换条件。

info在特殊输入模式有override时隐藏模型/behavior/permission/context，仍追加queued和transcript提示；普通模式显示模型+reasoning、behavior（plan细分phase）、always-approve优先auto再ask，context只在used与非零total均存在时显示，total优先实时再model窗口，percentage调用共享helper。提示按顺序set_line裁宽，无窄宽优先级重排。pending hint仅未过期且有label时显示双击快捷键提醒，不执行动作。

tail_height在reload返回0，否则从同scan_frontier.tail_start测所有后续项并累加gap，与draw_tail共享renderer/cwd/frame。测试已读btw整区边界、cwd和去accent列导致换行差异（fixture要求真有差异，非空泛断言）、reconnect replay暂停与只补漏条目的helper路径至925行；其尾段及其它测试待续。未运行动态测试。

## live 全文审阅完成

补读926–末尾：reconnect成功只提交staged tail、失败恢复旧frontier/丢弃partial replay，均使用pager test-support helper和返回true的writer闭包，不是网络断连真实终端测试。动画高度回归覆盖thinking/message/execute四种宽度，非所有block种类；状态Buffer断言idle/fullscreen hint、Responding/Retrying和loop watching；prompt样式只专测bash，info覆盖context/queue、bash隐藏context、transcript替代提示、behavior和permission标记；pending hint覆盖None/silent/过期。live文件全文闭合，未运行测试；剩余commit.rs及commit_tests.rs待阅读。


### commit.rs 与 commit_tests.rs（完整读取）

- 提交前沿按顺序遍历；等待用户输入和未 terminal 的 coordination 行始终阻塞；空闲时允许其余遗留 running 行，运行中仅已完成行、BgTask 和后面已有条目的 AgentMessage 可提交。已提交 ID 优先跳过，失败条目保留前沿；只在写入回调成功后标记，下一次调用清除失败标记重试。该机制不证明终端部分写入具有事务性。
- 测量前为 live tail 写入提交显示模式：Command notice 使用默认模式，coordination 折叠，Edit 展开，成功查询类折叠，其余工具截断，Thinking 由开关决定；克隆 appearance 关闭时间戳、左右 padding，启用 reasoning dim/italic 与展开提示。仅未折叠 thinking 保留 dim accent 列，共享 renderer 传入 owning cwd、frame、flat background。
- insert_committed 测量完整高度，零高度直接成功；非零 cap 限制 buffer 高度，完整布局后最后一行替换隐藏行数和 /transcript 提示。链接不保留覆盖 footer 的行；gap 常量为零。cap 限制缓冲区而不证明完整布局/链接计算开销有界。paint_committed 的 set_style 不清除旧 glyph；短 footer 后旧内容是否残留应另列验证，不能从注释认定已清空。
- app modal、session reload、无 active agent 或零宽度时不提交；实际回调在写入前 finish_running 并修改 display mode，写失败并不回滚这些字段。成功后仅折叠/截断项进入 expand 记录；返回值表示是否遇到失败前沿。已打印内容原地修改不会更新 native history。
- expand 队列在所有 guard 通过后消费；已移除 ID 跳过，设 Expanded 后无 cap 重印；失败保留当前与余下 ID 重排队，模式修改不回滚。不是修改原生历史中的旧副本。
- pending-input marks 在帧首同步，统一 sizing/commit 判断。commit_tests 全部 1358 行已读：回调失败/重试、去重、移除/rewind/clear、plan anchor、coordination、BgTask、notice、compaction replay、idle/pending 边界；Buffer 高度与 cap、cwd、native colors、diff 背景、accent、折叠策略。主要是状态/Buffer 测试，不等同真实终端 IO 故障或完整交互验证。


完整读取已闭合：12 个 Rust 文件及 Cargo.toml；功能已映射至 minimal-terminal delta。动态测试进行中，结果待附。

动态验证：cargo test --locked -p pager-minimal --all-features，禁用 incremental/dev/test debug，显式本工作树 target，退出0。86项测试通过，0失败/忽略，doctest0；日志 /tmp/grow-pager-minimal-tests.log。测试完成后立即清理本任务 target。未执行真实终端交互或部分IO失败注入。
