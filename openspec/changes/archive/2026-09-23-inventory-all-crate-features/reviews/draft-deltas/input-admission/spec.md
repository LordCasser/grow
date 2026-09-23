## ADDED Requirements

### Requirement: Eligible contiguous prompt combining
队列合并 SHALL 仅合并连续可参与的前缀：普通非 synthetic、非 skill 展开、非 bash 且 text 非空；首项可携带图片，后续项不得携带图片或处于 skip_ids。

#### Scenario: 中间遇到不合格项
- **WHEN** 普通文本后遇到图片、命令、展开 skill、synthetic 或编辑 hold 项
- **THEN** 合并在该项前停止，不越过它合并后面的输入。

#### Scenario: 空队列和不合格首项
- **WHEN** 队列为空或首项不能参与合并
- **THEN** 空队列返回长度 0；不合格首项返回长度 1。

证据：`crates/codegen/prompt-queue/src/combine.rs` — `combine_prefix_len`。

### Requirement: Combined prompt presentation metadata
文本合并 SHALL 用两个换行连接非空片段，至少两个展示片段时才写 combinedDisplayTexts 元数据。

#### Scenario: 多条输入展示
- **WHEN** 传入两个原始展示文本
- **THEN** stamp_combined_display_texts 写入字符串数组，可保留多个气泡的边界。

#### Scenario: 单条输入
- **WHEN** 只传一个展示片段
- **THEN** 不新增 combinedDisplayTexts；join_texts 跳过空字符串。

证据：`crates/codegen/prompt-queue/src/combine.rs` — `stamp_combined_display_texts`。

### Requirement: Structured queue wire state
QueueChanged SHALL 携带 session_id、队列项及可选运行中输入信息；QueueEntryWire 保留 id、version、owner、last_editor、kind、text、position 和组合文本，ForegroundSnapshot 明确携带 origin 与 turn_kind。

#### Scenario: 缺失路由身份
- **WHEN** 反序列化不含 sessionId 的 QueueChanged
- **THEN** 失败，不将广播默认路由到任意会话。

#### Scenario: 稀疏队列项
- **WHEN** 项只有 id，其他可默认字段缺失
- **THEN** 按类型默认值解析；None 的可选字段在序列化时省略，wire 键使用 camelCase。

证据：`crates/codegen/prompt-queue/src/types.rs` — `QueueChanged`；`crates/codegen/prompt-queue/src/types.rs` — `ForegroundSnapshot`。


### Requirement: Clipboard paste completion precedence

reduce_clipboard_paste_completion SHALL 先处理attachment：除FullMiss外的Handled、Dropped及Failed均原样短路；attachment为FullMiss时若提供file completion则原样采用file。没有file后，ClipboardKey文本读取失败返回TextRead失败；否则采用显式text insertion，若无显式值再采用BracketedInserted记录的同步结果，Inserted映射Handled、Empty映射FullMiss、Failed映射TargetInsertion。BracketedDeferred与ClipboardKey可在attachment/file miss后提供待插入文本，BracketedInserted不重复提供。

#### Scenario: Attachment wins paste
- **WHEN** attachment probe返回Handled、Dropped或Failed
- **THEN** 不再考虑file或text结果，直接返回attachment completion。

#### Scenario: Synchronous bracketed text
- **WHEN** attachment为FullMiss、无file且source为BracketedInserted
- **THEN** 按已记录Insertion归约结果，不再次请求插入其text。

源码证据：`crates/codegen/pager/src/app/actions.rs` — `ClipboardPasteSource / ClipboardPasteCompletion / reduce_clipboard_paste_completion`。


### Requirement: Pager fragmented CSI mouse and focus filtering

CsiFragmentFilter SHALL 仅把无modifier或Shift的Char Press送入状态机，识别`[<digits;digits;digitsM/m`及`[I/[O`。完整SGR片段连同同批次紧邻其前的bare Esc被删除；从`[<`起的深层partial保存在实例中跨filter调用续接，reject时先释放tentative并以当前字符重新尝试Idle起点。批尾仅停在`[`时立即释放，不能跨批holding typed bracket。Focus片段只有同批次存在此前bare Esc时转为FocusGained/FocusLost，时间戳取完成I/O事件；无此前Esc的typed `[I/[O`原样保留。单独Esc跨批先发出，因此跨批focus报告不会被重组。任何非候选事件都会释放partial并原样保留。

#### Scenario: Cross batch SGR body
- **WHEN** 首批含`[<`及部分数字、次批补齐字段与M/m
- **THEN** 整个fragment被过滤，不向输入消费者泄漏tentative字符。

#### Scenario: Same batch focus report
- **WHEN** 同一batch依次出现bare Esc、`[`和Shift-I或Shift-O
- **THEN** 撤回Esc和bracket，输出一个使用final事件时间戳的Focus事件。

#### Scenario: Cross batch focus limitation
- **WHEN** bare Esc单独在前一batch，`[I`在后一batch
- **THEN** Esc已无法撤回，后一批字符也原样输出，不重组成Focus事件。

源码证据：`crates/codegen/pager/src/app/csi_filter.rs` — `CsiFragmentFilter / CsiFragmentState / csi_filterable_char / tests`。
### Requirement: Pager multiline slash snapshot toggle

MultilineCommand SHALL 标记session_scoped且offered_when_session_less=true；run读取CommandExecCtx.pager_state.multiline_mode的快照并返回其取反值SetMultilineMode，不检查session、参数或当前screen mode。dashboard可通过session-less offering使用自身multiline flag，但该命令结构不携带owner身份；具体路由由统一执行层决定。命令本身不修改Enter/Shift+Enter映射、不持久化磁盘，也不保证快照读取与Action消费之间没有其他切换。

#### Scenario: Multiline snapshot off
- **WHEN** 执行上下文multiline_mode为false
- **THEN** 返回SetMultilineMode(true)。

#### Scenario: Sessionless offering
- **WHEN** 当前为dashboard且没有session
- **THEN** 元数据允许命令被提供；run仍只产生无owner的typed Action。

证据：`crates/codegen/pager/src/slash/commands/multiline.rs` — `MultilineCommand::session_scoped / offered_when_session_less / run`。

### Requirement: Pager slash leading invocation parsing and static completeness

parse_invocation SHALL 仅接受以`/`开头且后面存在非空command token的整行；只移除第一个slash，token截止首个Unicode whitespace并保持原大小写，参数取剩余部分的trim_start结果但保留尾随空白。空串、非slash及bare slash返回None。analyze_input同样只处理buffer开头slash，cursor收紧到text byte长度；slash之后全为空白时固定command range为0..1、空query且cursor_in_command=true，否则command range到首个whitespace，query只取slash后到cursor/command_end，cursor越过command_end才建立去除连续Unicode前导空白后的args range/query。is_command_complete使用parse结果与registry get_for_dispatch：无法解析返回false；未知、hard-hidden、restricted或tool-gated到无法lookup的token返回true以交dispatch处理；已知takes_args=false或args_required=false均为true，只有静态takes_args=true且args_required=true时要求trim后参数非空。takes_args_now不参与Enter完整性，menu-hidden命令仍按静态参数契约判断。

#### Scenario: Unknown command submission
- **WHEN** 输入/unknown或带参数的未知command
- **THEN** 完整性为true，让dispatch立即决定错误或fallback。

#### Scenario: Left trimmed arguments
- **WHEN** 输入command与参数之间有多个Unicode空白且参数尾部有空白
- **THEN** args去除所有前导空白但保留尾随空白。

#### Scenario: Cursor before existing args
- **WHEN** cursor回到command token或刚越过分隔符且buffer后方已有参数
- **THEN** query按cursor收紧，但args_query_is_empty依据完整args range内容而非仅cursor前缀。

源码证据：
- `crates/codegen/pager/src/slash/mod.rs` — `analyze_input / SlashInvocation / parse_invocation / is_command_complete`。

### Requirement: Pager inline slash token recognition argument regions and highlights

scan_inline_slash_tokens SHALL 扫描buffer任意位置的`/name`，slash必须在byte 0或其前一byte满足ASCII whitespace，name至少一个字符并延伸到下一个Unicode whitespace；因此word内部路径slash、bare slash及非ASCII空白直接前导的slash不成token。range使用UTF-8 byte offset并含slash，cursor经text长度收紧后以闭区间`start <= cursor <= end`标记has_cursor。mid_text_slash_context优先选择cursor落在token上的command phase；否则cursor在token结束后、下一slash token开始之前且紧随片段以Unicode whitespace开头时进入该token的args phase。args实际start跳过连续Unicode whitespace，end为下一token start或text末尾，suggest query只取start到cursor/end；只有registry menu lookup成功、command_offered且takes_args_now时生成args建议。recognized_token_ranges与提交echo共享scan结果，并仅保留registry.get可见且command_offered的token，所以menu-hidden与surface/mode被抑制命令不高亮；leading command recognition另用get_for_dispatch，menu-hidden命令在buffer开头可被识别但不进入mid-text高亮。非slash普通文本的snapshot只保留recognized ranges；ghost由当前dropdown selection单独产生。

#### Scenario: Path slash ignored
- **WHEN** 文本包含foo/bar/baz.rs
- **THEN** 内部slash前一byte不是ASCII whitespace，不生成token。

#### Scenario: Second token argument boundary
- **WHEN** 文本含`/first args /second`且cursor在first参数区域
- **THEN** first args range截止second slash的byte起点，不吞掉第二个token。

#### Scenario: Session scoped highlight suppressed
- **WHEN** dashboard文本同时含session-scoped /compact和global /theme
- **THEN** recognized ranges只包含/theme，且cursor落在/compact时command_recognized=false。

源码证据：
- `crates/codegen/pager/src/slash/mod.rs` — `InlineSlashToken / scan_inline_slash_tokens / mid_text_slash_context / next_slash_token_start / refresh_mid_text_slash / refresh_mid_text_slash_args / recognized_token_ranges`。

### Requirement: Pager modified Enter terminal rescue and macOS modifier snapshot

macOS macos_modifiers::snapshot SHALL 通过一次CoreGraphics CGEventSourceFlagsState(HIDSystemState=1)读取全局flags，并按公开bitmask投影command、option、shift、control四个布尔值；该模块只在target_os=macos编译，非macOS不链接CoreGraphics。is_apple_terminal_newline_modifier_held先读取共享terminal_context，只有keyboard_capabilities.enter_needs_rescue=true时才查询OS；macOS把shift/option/command任一按下视为true，control单独不算newline modifier，非macOS恒false。is_mod_enter必须KeyCode::Enter，随后只接受crossterm ALT或SHIFT，或上述Apple Terminal rescue；SUPER/Cmd flag、CONTROL、BackTab、Shift+Tab及其他字符键不直接匹配。Cmd+Enter仅能在terminal丢失flag而呈bare Enter时通过CoreGraphics救援，不建立全terminal SUPER+Enter语义。

#### Scenario: Shift Enter
- **WHEN** KeyCode为Enter且crossterm modifiers含SHIFT
- **THEN** 不依赖terminal rescue直接识别为modified Enter。

#### Scenario: Apple bare Enter rescue
- **WHEN** terminal标记enter_needs_rescue且CoreGraphics快照显示Command按下
- **THEN** bare Enter识别为newline chord。

#### Scenario: Shift Tab exclusion
- **WHEN** KeyCode为BackTab或Tab加SHIFT
- **THEN** 始终返回false，不把通用modifier误当Enter。

源码证据：
- `crates/codegen/pager/src/input/mod.rs` — module exports。
- `crates/codegen/pager/src/input/macos_modifiers.rs` — `CGEventSourceFlagsState / snapshot`。
- `crates/codegen/pager/src/input/terminal_support.rs` — `is_apple_terminal_newline_modifier_held / is_mod_enter / os_any_newline_modifier_held`。

### Requirement: Pager key shortcut ASCII case normalization matching and labels

KeyShortcut::new SHALL 对Char执行ASCII case规范化：uppercase code自动加入SHIFT，任何带SHIFT的lowercase code改为uppercase；非Char保持。matches拒绝Release，Press与Repeat均在对event执行同一规范化后要求code与完整modifiers精确相等，因此多余modifier不匹配。to_key_event生成Press事件。is_letter_or_shift_letter仅当code为ASCII字母且modifier为空或精确SHIFT时true。compact Display按SUPER、CONTROL、ALT、SHIFT顺序输出平台标签，macOS用Cmd/Opt、其他用Super/Alt，Char最终ASCII lowercase；Backspace/Delete/Page键用短名，箭头用glyph。pretty Display使用Cmd|Super、Ctrl、Opt|Alt、Shift的`+`分段与完整键名，BackTab缺SHIFT bit时补一个Shift。key!宏支持char、named KeyCode及函数键并统一走KeyShortcut::new。

#### Scenario: Uppercase equivalence
- **WHEN** shortcut由Char G无modifier或Char g加SHIFT构造
- **THEN** 两者规范化为Char G加SHIFT并能匹配同一非Release事件。

#### Scenario: Exact modifiers
- **WHEN** 目标Ctrl+v但事件额外携带ALT
- **THEN** 不匹配Ctrl+v shortcut。

#### Scenario: Bare letter gate
- **WHEN** code为ASCII letter但modifier含CONTROL
- **THEN** is_letter_or_shift_letter返回false。

源码证据：
- `crates/codegen/pager/src/input/key.rs` — `KeyShortcut::new / normalize_case / matches / to_key_event / display / display_pretty / is_letter_or_shift_letter / Display / key!`。

### Requirement: Pager paste shift tab undo AltGr and text key predicates

input key predicates SHALL 使用KeyShortcut精确modifier匹配。is_paste_key接受Ctrl+v或Super+v，Windows额外接受裸Alt+v以绕过Windows Terminal文本paste拦截，其他平台不接受Alt+v；Ctrl+Alt的AltGr+v因多modifier不匹配。is_inline_paste_key只接受Ctrl+Shift+v或Super+Shift+v。is_undo_key完全委托ratatui_textarea::is_undo_input，使Ctrl/Cmd+z与实际editor绑定一致而redo不算undo。shift_tab_keys固定覆盖BackTab无modifier、BackTab+SHIFT、Tab+SHIFT，is_shift_tab对三种使用matches并排除Release与plain Tab。Windows is_altgr只检查modifier集合包含CONTROL|ALT，因此可同时有其他bits；非Windows恒false。is_text_input_key要求Char且modifier为空、精确SHIFT或Windows is_altgr；该函数自身不检查KeyEventKind，因此Release Char也可在此分类为text，消费方仍需事件阶段gate。Enter和带Ctrl/Alt/Super的普通Char不算text，Windows含Ctrl+Alt的Char例外。

#### Scenario: Windows image paste escape
- **WHEN** 平台为Windows且收到裸Alt+v
- **THEN** 识别为paste；同事件在macOS/Linux不识别。

#### Scenario: Shift Tab wire variants
- **WHEN** terminal发送BackTab、BackTab+SHIFT或Tab+SHIFT Press
- **THEN** 三者均识别为Shift+Tab，Release均不匹配。

#### Scenario: AltGr text
- **WHEN** Windows Char事件modifiers包含Ctrl和Alt
- **THEN** 按AltGr text input接收；其他平台拒绝该组合。

源码证据：
- `crates/codegen/pager/src/input/key.rs` — `is_paste_key / is_inline_paste_key / is_undo_key / is_altgr / shift_tab_keys / is_shift_tab / is_text_input_key`。

### Requirement: Pager keyboard normalizer raw Ctrl B and dropped deletion modifier rescue

KeyboardNormalizer SHALL 以ModifierProbe和terminal ModifierDelivery构造；production probe在macOS调用CoreGraphics snapshot，其他OS返回全false，from_terminal_context固定捕获当前terminal modifier_delivery。rescue_key首先无条件把code U+0002且无modifier的KeyEvent规范化为Char b+CONTROL并保留其他event字段，不查询probe也不要求delivery受益；其余路径只有delivery整体benefits_from_rescue、event完全无modifier且code为Backspace或Delete时才probe。物理Command且cmd axis可救援时加SUPER并优先于Option；否则物理Option且opt axis可救援时加ALT；Shift/Control不参与，未按下或对应axis为Native时None。已有任何modifier、普通字符、Enter/Esc/Tab/arrow均不改。rescue只对Event::Key且实际升级时返回Owned Event，其余返回Borrowed；它不消费事件或执行删除，只让所有下游surface看到canonical modifiers。

#### Scenario: Raw control byte
- **WHEN** terminal发送无modifier的Char U+0002
- **THEN** 无论delivery分类都改为Ctrl+b，避免当作text input。

#### Scenario: Command precedence
- **WHEN** bare Backspace、物理Command与Option同时按下且两axis均Dropped
- **THEN** 只添加SUPER，不添加ALT。

#### Scenario: Native axis
- **WHEN** 物理Command按下但delivery声明cmd为Native
- **THEN** 不伪造SUPER；若Option axis可救援且Option也按下才可添加ALT。

源码证据：
- `crates/codegen/pager/src/input/keyboard_normalizer.rs` — `ModifierState / ModifierProbe / OsModifierProbe / KeyboardNormalizer::from_terminal_context / rescue_key / rescue`。

### Requirement: Pager single line editor sanitation paste limits and grapheme deletion

LineEditor SHALL 以ratatui_textarea EditBuffer保存UTF-8文本与byte cursor。sanitize_single_line只移除CR和LF，不trim且不移除其他Unicode line separator/control；set_text以清理后文本替换整个buffer并把cursor置末尾，reset清空。insert_paste先清理换行再逐Unicode scalar应用allow_insert，收集最多max_chars个被允许字符；空accepted返回HandledNoChange，否则在当前cursor插入。byte-limit版本以max_total_bytes减当前完整buffer byte长度计算remaining，按原顺序收集能连续装入的完整Unicode scalar，首个超限字符即停止而不跳到后续较短字符；不拆UTF-8但不保证完整grapheme。delete_last_grapheme忽略原cursor，先移到文本末尾再删除一个向后grapheme cluster。insert_paste无policy等价允许全部且无字符数上限。所有方法只编辑内存，不验证展示宽度、内容安全或业务字段规则。

#### Scenario: Multiline paste
- **WHEN** paste文本含CR/LF和其他普通字符
- **THEN** 删除CR/LF后在cursor插入，其余字符顺序保持。

#### Scenario: UTF8 byte cap
- **WHEN** 现有文本2 bytes、总上限5且paste为中x
- **THEN** 只插入3-byte中，x因remaining耗尽不插入。

#### Scenario: Delete final emoji cluster
- **WHEN** cursor在开头且文本末尾为包含skin tone与ZWJ的emoji
- **THEN** 先跳到末尾并整cluster删除。

源码证据：
- `crates/codegen/pager/src/input/line_editor.rs` — `LineEditor::set_text / reset / insert_paste_with_policy / insert_paste_with_byte_limit / delete_last_grapheme / sanitize_single_line`。

### Requirement: Pager single line editor key mapping outcomes and viewport

LineEditor::handle_key SHALL 拒绝Release为Unhandled；Home无论modifier及精确Super+Left映射MoveLogicalLineStart，End无论modifier及精确Super+Right映射MoveLogicalLineEnd，其余交ratatui_textarea::classify_key_event。无法分类为Unhandled；分类为Insert时先调用一次allow_insert，拒绝则HandledNoChange，不应用buffer command；其他EditCommand直接apply。EditOutcome::Unchanged映射HandledNoChange，CursorOnly映射CursorChanged，TextOnly或TextAndCursor统一TextChanged。由共享editor继承word navigation/deletion与Ctrl+u等语义，本层不重写；测试锁定Alt/Control Left在hello-world从末尾移到hello-之后、Alt+Backspace删world、Ctrl+u只删cursor之前。viewport(width)直接返回grapheme-aware SingleLineViewport，visible_byte_range保持UTF-8边界并以display column表达cursor。

#### Scenario: Home at start
- **WHEN** cursor已在line start并再次按Home
- **THEN** 事件被识别但返回HandledNoChange。

#### Scenario: Rejected insertion
- **WHEN** classify得到Insert字符但allow_insert返回false
- **THEN** 不改文本或cursor并返回HandledNoChange。

#### Scenario: Wide grapheme viewport
- **WHEN** 窄viewport包含多column emoji grapheme
- **THEN** visible byte range不切开grapheme且cursor column按显示宽度计算。

源码证据：
- `crates/codegen/pager/src/input/line_editor.rs` — `LineEditOutcome / LineEditor::handle_key / handle_key_with_insert_policy / viewport / from_edit_outcome`。
### Requirement: Pager root interjection optimistic-echo deduplication

A `grow/session/interjection` notification SHALL parse JSON with string sessionId and text, optionally read a string interjectionId, and accept only a Root session match. When the root consumes the id from its self-interjection set, the broadcast echo is dropped and false returned; otherwise an interjection prompt block is appended and parent active status returned. Missing or null ids always render, Child matches are rejected, and malformed or incomplete payloads return false. This function does not deduplicate legacy id-less retransmissions.

#### Scenario: Remote interjection
- **WHEN** a root notification has an id not minted locally
- **THEN** its text is appended to scrollback.

#### Scenario: Self echo
- **WHEN** the id exists in the root's optimistic set
- **THEN** the id is consumed and no second block is appended.

#### Scenario: Legacy
- **WHEN** interjectionId is absent or null
- **THEN** the block always renders.

#### Scenario: Child
- **WHEN** session matching yields Child
- **THEN** the notification is dropped.

#### Scenario: Invalid
- **WHEN** JSON, sessionId or text is invalid
- **THEN** false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle_interjection`。

### Requirement: Pager prompt submission block metadata and reconciliation effects

Plain, structured-block and bash submissions SHALL send ACP PromptRequest with the owning session, promptId and optional screenMode. Plain text SHALL carry skillTokenRanges only when non-empty; bash SHALL use PromptBlockMeta::bash; block/image submission SHALL materialize image blocks off the async runtime and fail locally before ACP send if loading fails. All three SHALL return PromptResponse with the prompt id and an HTTP status extracted from ACP errors, although bash preserves the raw ACP error string while the other paths use format_acp_error. Prompt status reconciliation SHALL query grow/queue/prompt_status by session and prompt id. This file does not prove range validity, image decoding, server-side prompt idempotency, request ordering, prompt execution or reducer handling of late responses.

#### Scenario: Plain prompt metadata
- **WHEN** skill token ranges are non-empty
- **THEN** the single text block includes skillTokenRanges pairs; empty ranges leave block meta absent.

#### Scenario: Screen mode correlation
- **WHEN** a session screen mode is available
- **THEN** prompt request metadata includes both promptId and screenMode; default test flags omit screenMode.

#### Scenario: Image materialization failure
- **WHEN** append_prompt_images or its blocking task fails
- **THEN** PromptResponse is returned without sending the ACP prompt and http_status is None.

#### Scenario: Prompt RPC failure
- **WHEN** ACP rejects a submitted prompt
- **THEN** PromptResponse preserves prompt correlation and the extracted HTTP status when available.

#### Scenario: Status parse failure
- **WHEN** prompt_status returns invalid JSON
- **THEN** PromptStatusResolved carries a sanitized invalid-response error.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/agent/mvp_agent/prompt_response_meta_tests.rs agent bootstrap, model, and session control contract

crates/codegen/shell/src/agent/mvp_agent/prompt_response_meta_tests.rs SHALL 维护 agent bootstrap, model, and session control 的入口 args, includes_baseline_keys_without_usage, enriches_meta_with_camelcase_token_keys, preserves_zero_token_values, usage_object_lands_on_meta, cancel_trigger_lands_as_camelcase_meta_key, structured_output_maps_to_camelcase_meta_keys。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** args, includes_baseline_keys_without_usage, enriches_meta_with_camelcase_token_keys, preserves_zero_token_values, usage_object_lands_on_meta, cancel_trigger_lands_as_camelcase_meta_key, structured_output_maps_to_camelcase_meta_keys 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/agent/mvp_agent/prompt_response_meta_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/extensions/prompt_history.rs extension method and user-facing command boundary contract

crates/codegen/shell/src/extensions/prompt_history.rs SHALL 维护 extension method and user-facing command boundary 的入口 MAX_CONCURRENT_READS, MAX_HISTORY_SESSIONS, MAX_HISTORY_ENTRIES, PromptHistoryRequest, PromptHistoryResponse, handle, handle_prompt_history, load_prompts, load_bash_prompts, load_records, load_summaries, deduplicate。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MAX_CONCURRENT_READS, MAX_HISTORY_SESSIONS, MAX_HISTORY_ENTRIES, PromptHistoryRequest, PromptHistoryResponse, handle, handle_prompt_history, load_prompts, load_bash_prompts, load_records, load_summaries, deduplicate 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** MAX_CONCURRENT_READS, MAX_HISTORY_SESSIONS, MAX_HISTORY_ENTRIES, PromptHistoryRequest, PromptHistoryResponse, handle, handle_prompt_history, load_prompts, load_bash_prompts, load_records, load_summaries, deduplicate 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

证据：`crates/codegen/shell/src/extensions/prompt_history.rs`。

### Requirement: Shell crates/codegen/shell/src/extensions/prompt_meta.rs extension method and user-facing command boundary contract

crates/codegen/shell/src/extensions/prompt_meta.rs SHALL 维护 extension method and user-facing command boundary 的入口 PromptBlockMeta, bash, from_value, bash_roundtrip_serde, from_value_parses_canonical_shape, from_value_unrelated_meta, from_value_empty_object, from_value_rejects_unknown_fields。实现显示该边界包含 serde-backed wire/config types、platform or feature-gated branches、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/extensions/prompt_meta.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/prompt_build.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/prompt_build.rs SHALL 维护 session actor lifecycle and notifications 的入口 pick_user_image_url, conversation_has_project_instructions, is_project_instructions, LARGE_PROMPT_THRESHOLD, TRUNCATED_PROMPT_PREFIX_SIZE, LARGE_QUERY_BUDGET_PERCENT, BOUNDED_TAIL_BUDGET, SKILL_INLINE_BUDGET, ELISION_MARKER, OFFLOAD_NOTICE_MARKER, OFFLOAD_FAILED_NOTICE, truncate_bytes_suffix, bound_head_tail, build_offload_notice, build_truncated_prompt_message, strip_offload_notice, write_offload_and_build, rewrite_zero_turn_prefix (plus 5 additional private symbols)。实现显示该边界包含 explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary、hook dispatch or hook source boundary、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** pick_user_image_url, conversation_has_project_instructions, is_project_instructions, LARGE_PROMPT_THRESHOLD, TRUNCATED_PROMPT_PREFIX_SIZE, LARGE_QUERY_BUDGET_PERCENT, BOUNDED_TAIL_BUDGET, SKILL_INLINE_BUDGET, ELISION_MARKER, OFFLOAD_NOTICE_MARKER, OFFLOAD_FAILED_NOTICE, truncate_bytes_suffix, bound_head_tail, build_offload_notice, build_truncated_prompt_message, strip_offload_notice, write_offload_and_build, rewrite_zero_turn_prefix (plus 5 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/session/actor/prompt_build.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/prompt_queue.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/prompt_queue.rs SHALL 维护 session actor lifecycle and notifications 的入口 RunningPromptDisplay, queue_input, follow_up_promotion_eligible, queue_text_from_blocks, build_queue_wire, broadcast_queue_changed, broadcast_queue_changed_promoting, running_display_from_item, broadcast_queue_changed_inner, is_running_prompt, respond_removed_prompt, handle_remove_queued_prompt, handle_steer_queued_prompt, replace_queued_prompt_with_admitted_input, handle_reorder_queue, handle_clear_queue, handle_edit_queued_prompt, combine_front_pending_inputs (plus 20 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** RunningPromptDisplay, queue_input, follow_up_promotion_eligible, queue_text_from_blocks, build_queue_wire, broadcast_queue_changed, broadcast_queue_changed_promoting, running_display_from_item, broadcast_queue_changed_inner, is_running_prompt, respond_removed_prompt, handle_remove_queued_prompt, handle_steer_queued_prompt, replace_queued_prompt_with_admitted_input, handle_reorder_queue, handle_clear_queue, handle_edit_queued_prompt, combine_front_pending_inputs (plus 20 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** RunningPromptDisplay, queue_input, follow_up_promotion_eligible, queue_text_from_blocks, build_queue_wire, broadcast_queue_changed, broadcast_queue_changed_promoting, running_display_from_item, broadcast_queue_changed_inner, is_running_prompt, respond_removed_prompt, handle_remove_queued_prompt, handle_steer_queued_prompt, replace_queued_prompt_with_admitted_input, handle_reorder_queue, handle_clear_queue, handle_edit_queued_prompt, combine_front_pending_inputs (plus 20 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/prompt_queue.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/prompt_suggest.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/prompt_suggest.rs SHALL 维护 session timeline and state model 的入口 next, effective_suggest_model, TRANSCRIPT_BUDGET_CHARS, MESSAGE_CAP_CHARS, SUGGESTION_MAX_CHARS, SUGGESTION_MAX_WORDS, ONE_WORD_ALLOWLIST, SUGGEST_PROMPT_SYSTEM, that, transcript_line, build_transcript, suggest_prompt_user_message, REPEAT_MIN_WORDS, normalize_for_repeat, is_repeat_of_user_message, sanitize_suggestion, effective_model_without_configuration_is_disabled, effective_model_client_hint_beats_default_and_is_guarded (plus 23 additional private symbols)。实现显示该边界包含 explicit error/result paths、platform or feature-gated branches、session/timeline state projection、git/worktree context、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** next, effective_suggest_model, TRANSCRIPT_BUDGET_CHARS, MESSAGE_CAP_CHARS, SUGGESTION_MAX_CHARS, SUGGESTION_MAX_WORDS, ONE_WORD_ALLOWLIST, SUGGEST_PROMPT_SYSTEM, that, transcript_line, build_transcript, suggest_prompt_user_message, REPEAT_MIN_WORDS, normalize_for_repeat, is_repeat_of_user_message, sanitize_suggestion, effective_model_without_configuration_is_disabled, effective_model_client_hint_beats_default_and_is_guarded (plus 23 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/helpers/prompt_suggest.rs`。

### Requirement: Shell crates/codegen/shell/src/session/input_inbox.rs session timeline and state model contract

crates/codegen/shell/src/session/input_inbox.rs SHALL 维护 session timeline and state model 的入口 ARTIFACT_DIRECTORY, IMAGE_DIRECTORY, MAX_IMAGE_BLOB_BYTES, MAX_INPUT_IMAGE_BYTES, MAX_INPUT_IMAGES, ORPHAN_SWEEP_BATCH_SIZE, InputPayload, StoredBlock, deserialize, ImageReference, ImageBlobRef, try_map_blocks, LimitedWriter, write, flush, serialized_size, bounded_json, validate_payload_bounds (plus 37 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ARTIFACT_DIRECTORY, IMAGE_DIRECTORY, MAX_IMAGE_BLOB_BYTES, MAX_INPUT_IMAGE_BYTES, MAX_INPUT_IMAGES, ORPHAN_SWEEP_BATCH_SIZE, InputPayload, StoredBlock, deserialize, ImageReference, ImageBlobRef, try_map_blocks, LimitedWriter, write, flush, serialized_size, bounded_json, validate_payload_bounds (plus 37 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** ARTIFACT_DIRECTORY, IMAGE_DIRECTORY, MAX_IMAGE_BLOB_BYTES, MAX_INPUT_IMAGE_BYTES, MAX_INPUT_IMAGES, ORPHAN_SWEEP_BATCH_SIZE, InputPayload, StoredBlock, deserialize, ImageReference, ImageBlobRef, try_map_blocks, LimitedWriter, write, flush, serialized_size, bounded_json, validate_payload_bounds (plus 37 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/input_inbox.rs`。

### Requirement: Shell crates/codegen/shell/src/session/prompt_parser.rs session timeline and state model contract

crates/codegen/shell/src/session/prompt_parser.rs SHALL 维护 session timeline and state model 的入口 ParsedPrompt, assemble, assemble_parts_with_skills, parse_prompt_with_skills, render_message, collect_file_references, CursorPosition, FileState, EditorMeta, parse_editor_meta, extract_path_from_uri, render_regular_links, render_resource_links_grow, render_focused_files, render_open_files, render_grow, test_collect_single_reference, test_collect_multiple_references (plus 17 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ParsedPrompt, assemble, assemble_parts_with_skills, parse_prompt_with_skills, render_message, collect_file_references, CursorPosition, FileState, EditorMeta, parse_editor_meta, extract_path_from_uri, render_regular_links, render_resource_links_grow, render_focused_files, render_open_files, render_grow, test_collect_single_reference, test_collect_multiple_references (plus 17 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/prompt_parser.rs`。

### Requirement: Shell crates/codegen/shell/src/session/prompt_queue.rs session timeline and state model contract

crates/codegen/shell/src/session/prompt_queue.rs SHALL 维护 session timeline and state model 的入口 QUEUE_CHANGED_METHOD, PromptStatus, RecentPromptTerminal, queue_changed_serializes_camel_case_with_session_id, queue_changed_round_trips_running_prompt_id, queue_entry_wire_round_trips_last_editor。实现显示该边界包含 serde-backed wire/config types、channel or acknowledgement flow、platform or feature-gated branches、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Async lifecycle
- **WHEN** QUEUE_CHANGED_METHOD, PromptStatus, RecentPromptTerminal, queue_changed_serializes_camel_case_with_session_id, queue_changed_round_trips_running_prompt_id, queue_entry_wire_round_trips_last_editor 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/prompt_queue.rs`。

### Requirement: Shell crates/codegen/shell/src/session/prompt_timing.rs session timeline and state model contract

crates/codegen/shell/src/session/prompt_timing.rs SHALL 维护 session timeline and state model 的入口 the file module entrypoint。实现显示该边界包含 session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 the file module entrypoint 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/prompt_timing.rs`。
### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs interactive user question protocol contract
crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs SHALL implement the interactive user question protocol boundary through validate question/options and timeout policy, encode ACP extension requests, and format accepted/cancelled responses. Its source symbols CANCEL_TEXT, format_accepted_tool_result, format_id_keyed_accepted_tool_result, format_chat_about_this, format_skip_interview, make_question, format_accepted_single_no_annotations, format_accepted_multiple_with_annotations, format_accepted_multi_select, format_accepted_freeform_only, format_accepted_preview_and_notes, format_accepted_empty, format_accepted_partial, id_keyed_q, format_id_keyed_single_question_single_select_matches_capture, format_id_keyed_three_questions_matches_capture, format_id_keyed_multi_select_inferred_csv, format_id_keyed_unanswered_questions_are_omitted (additional symbols omitted from the title but included in source evidence) follow explicit markers platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `CANCEL_TEXT`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_tool_result`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_id_keyed_accepted_tool_result`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_chat_about_this`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_skip_interview`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `make_question`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_single_no_annotations`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_multiple_with_annotations`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_multi_select`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_freeform_only`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_preview_and_notes`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_empty`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_partial`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `id_keyed_q`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_id_keyed_single_question_single_select_matches_capture`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_id_keyed_three_questions_matches_capture`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_id_keyed_multi_select_inferred_csv`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_id_keyed_unanswered_questions_are_omitted`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_id_keyed_no_answers_emits_header_only`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_id_keyed_freeform_dismissal_uses_notes_without_selected_prefix`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_id_keyed_freeform_without_notes_is_dropped`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_accepted_special_chars`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_chat_about_this_mixed`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_chat_about_this_all_answered`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_chat_about_this_none_answered`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_skip_interview_all_answered`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_skip_interview_mixed`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/format.rs` — `format_skip_interview_no_indentation`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs interactive user question protocol contract
crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs SHALL implement the interactive user question protocol boundary through validate question/options and timeout policy, encode ACP extension requests, and format accepted/cancelled responses. Its source symbols RESPONSE_TIMEOUT, DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED, AskUserQuestionParams, default, wait_budget, QuestionOption, Question, AskUserQuestionInput, AskUserQuestionTool, kind, tool_namespace, emitted_notifications, str, description_template, their, requires_expr, Args, Output (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `RESPONSE_TIMEOUT`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `AskUserQuestionParams`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `default`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `wait_budget`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `QuestionOption`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `Question`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `AskUserQuestionInput`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `AskUserQuestionTool`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `emitted_notifications`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `their`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `make_question`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `timeout_secs`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `resources_with_sender`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `resources_with_sender_and_params`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `tool_name_and_description`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` — `tool_access_is_internal_control`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs interactive user question protocol contract
crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs SHALL implement the interactive user question protocol boundary through validate question/options and timeout policy, encode ACP extension requests, and format accepted/cancelled responses. Its source symbols QuestionAnnotation, AskUserQuestionMode, AskUserQuestionExtRequest, AskUserQuestionExtResponse, UserQuestionResult, UserQuestionResponse, UserQuestionError, UserQuestionRequest, UserQuestionSender, into_response, sample_questions, mode_serializes_as_snake_case, mode_round_trips, ext_request_serializes_camel_case, ext_request_round_trips, ext_response_accepted_serializes_tagged, ext_response_accepted_omits_empty_annotations, ext_response_chat_about_this_serializes (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、channel, fanout, or acknowledgement flow、timeout, budget, or rate limit、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `QuestionAnnotation`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `AskUserQuestionMode`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `AskUserQuestionExtRequest`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `AskUserQuestionExtResponse`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `UserQuestionResult`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `UserQuestionResponse`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `UserQuestionError`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `UserQuestionRequest`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `UserQuestionSender`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `into_response`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `sample_questions`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `mode_serializes_as_snake_case`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `mode_round_trips`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `ext_request_serializes_camel_case`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `ext_request_round_trips`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `ext_response_accepted_serializes_tagged`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `ext_response_accepted_omits_empty_annotations`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `ext_response_chat_about_this_serializes`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `ext_response_skip_interview_serializes`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `ext_response_cancelled_serializes`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `ext_response_round_trips_all_variants`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `into_response_accepted`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `into_response_chat_about_this_carries_questions`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `into_response_skip_interview_carries_questions`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `into_response_cancelled`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `annotation_omits_none_fields`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `annotation_includes_present_fields`；`crates/codegen/tools/src/implementations/grow_build/ask_user_question/types.rs` — `deserialize_accepted_rejects_string_answer`。

### Requirement: Tools crates/codegen/tools/src/interjection/buffer.rs interjection buffer and event formatting contract
crates/codegen/tools/src/interjection/buffer.rs SHALL implement the interjection buffer and event formatting boundary through buffer, format, and emit interjection events while preserving ordering and ownership. Its source symbols PendingInterjection, FormattedInterjection, InterjectionBuffer, drain_formatted, drain_formatted_sanitizes_wraps_and_preserves_order follow explicit markers serde/json wire or configuration、platform/feature conditional、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/interjection/buffer.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/interjection/buffer.rs` — `PendingInterjection`；`crates/codegen/tools/src/interjection/buffer.rs` — `FormattedInterjection`；`crates/codegen/tools/src/interjection/buffer.rs` — `InterjectionBuffer`；`crates/codegen/tools/src/interjection/buffer.rs` — `drain_formatted`；`crates/codegen/tools/src/interjection/buffer.rs` — `drain_formatted_sanitizes_wraps_and_preserves_order`。

### Requirement: Tools crates/codegen/tools/src/interjection/events.rs interjection buffer and event formatting contract
crates/codegen/tools/src/interjection/events.rs SHALL implement the interjection buffer and event formatting boundary through buffer, format, and emit interjection events while preserving ordering and ownership. Its source symbols EventQueueInner, EventQueue, clone, default, new, push, push_capped, len, is_empty, drain_matching, drain_all, clear, wait_nonempty, lock, snapshot, push_and_len, clones_share_one_queue, push_capped_drops_oldest (additional symbols omitted from the title but included in source evidence) follow explicit markers async task and cancellation lifecycle、platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/interjection/events.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/interjection/events.rs` — `EventQueueInner`；`crates/codegen/tools/src/interjection/events.rs` — `EventQueue`；`crates/codegen/tools/src/interjection/events.rs` — `clone`；`crates/codegen/tools/src/interjection/events.rs` — `default`；`crates/codegen/tools/src/interjection/events.rs` — `new`；`crates/codegen/tools/src/interjection/events.rs` — `push`；`crates/codegen/tools/src/interjection/events.rs` — `push_capped`；`crates/codegen/tools/src/interjection/events.rs` — `len`；`crates/codegen/tools/src/interjection/events.rs` — `is_empty`；`crates/codegen/tools/src/interjection/events.rs` — `drain_matching`；`crates/codegen/tools/src/interjection/events.rs` — `drain_all`；`crates/codegen/tools/src/interjection/events.rs` — `clear`；`crates/codegen/tools/src/interjection/events.rs` — `wait_nonempty`；`crates/codegen/tools/src/interjection/events.rs` — `lock`；`crates/codegen/tools/src/interjection/events.rs` — `snapshot`；`crates/codegen/tools/src/interjection/events.rs` — `push_and_len`；`crates/codegen/tools/src/interjection/events.rs` — `clones_share_one_queue`；`crates/codegen/tools/src/interjection/events.rs` — `push_capped_drops_oldest`；`crates/codegen/tools/src/interjection/events.rs` — `drain_matching_returns_matched_retains_rest_fifo`；`crates/codegen/tools/src/interjection/events.rs` — `push_capped_under_limit_keeps_all`；`crates/codegen/tools/src/interjection/events.rs` — `drain_matching_none_match_retains_all`；`crates/codegen/tools/src/interjection/events.rs` — `drain_matching_on_empty_is_empty`；`crates/codegen/tools/src/interjection/events.rs` — `drain_all_empties_in_fifo_order`；`crates/codegen/tools/src/interjection/events.rs` — `clear_discards_all`；`crates/codegen/tools/src/interjection/events.rs` — `snapshot_reads_without_draining`；`crates/codegen/tools/src/interjection/events.rs` — `wait_nonempty_wakes_on_push`；`crates/codegen/tools/src/interjection/events.rs` — `wait_nonempty_returns_for_existing_event`；`crates/codegen/tools/src/interjection/events.rs` — `wait_nonempty_wakes_all_registered_waiters`。

### Requirement: Tools crates/codegen/tools/src/interjection/format.rs interjection buffer and event formatting contract
crates/codegen/tools/src/interjection/format.rs SHALL implement the interjection buffer and event formatting boundary through buffer, format, and emit interjection events while preserving ordering and ownership. Its source symbols LARGE_PROMPT_THRESHOLD, user_query, format_interjection, wraps_in_user_query_with_midturn_note, truncates_at_utf8_boundary, short_text_untouched follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/interjection/format.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/interjection/format.rs` — `LARGE_PROMPT_THRESHOLD`；`crates/codegen/tools/src/interjection/format.rs` — `user_query`；`crates/codegen/tools/src/interjection/format.rs` — `format_interjection`；`crates/codegen/tools/src/interjection/format.rs` — `wraps_in_user_query_with_midturn_note`；`crates/codegen/tools/src/interjection/format.rs` — `truncates_at_utf8_boundary`；`crates/codegen/tools/src/interjection/format.rs` — `short_text_untouched`。

### Requirement: Tools crates/codegen/tools/src/interjection/mod.rs interjection buffer and event formatting contract
crates/codegen/tools/src/interjection/mod.rs SHALL implement the interjection buffer and event formatting boundary through buffer, format, and emit interjection events while preserving ordering and ownership. Its source symbols mod follow explicit markers tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/interjection/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/interjection/mod.rs` — `mod`。
