## ADDED Requirements

### Requirement: Markdown rendering entrypoints
Markdown SHALL 提供 ANSI 字符串与 SourceMap、ratatui 完整输出与 checkpoint、简化 lines 与 line map，以及可复用 MarkdownBuffers 和可选表格宽度的入口；一次性入口先规范化 LaTeX 分隔符，坐标属于规范化后的输入。

#### Scenario: 完整输出
- **WHEN** 调用 render_markdown_ratatui_full
- **THEN** 返回样式行、行源映射、超链接与闭合 fenced code metadata；裸 URL 后扫描并按行列排序。

#### Scenario: ANSI 入口
- **WHEN** 调用 render_markdown
- **THEN** 同样先规范化 LaTeX，但不执行 ratatui 的裸 URL metadata 后扫描；不产生 OSC 8。

证据：`crates/codegen/markdown/src/lib.rs` — `render_markdown_ratatui_with_buffers_width`；`crates/codegen/markdown/src/lib.rs` — `render_markdown`。

### Requirement: Markdown parser policy
MarkdownParser SHALL 使用统一的 GFM、表格、任务列表、数学和删除线策略；仅双波浪线产生删除线事件，单波浪线保持字面文本。

#### Scenario: 单波浪线
- **WHEN** 输入 ~word~
- **THEN** 不作为删除线；~~word~~ 仍使用删除线样式。

#### Scenario: 低层 parser
- **WHEN** 直接构造 MarkdownParser
- **THEN** 解析其收到的文本并清理复用 buffers；不自动执行高层 LaTeX 规范化。

证据：`crates/codegen/markdown/src/parser_policy.rs` — `DoubleTildeOnlyStrike`；`crates/codegen/markdown/src/parse.rs` — `pub fn parse`。

### Requirement: Markdown style composition
MarkdownStyle SHALL 分别配置六级 heading 内外、强调、代码、列表、引用、任务、链接、表格与数学样式；Default 为无修饰样式。组合按次序覆盖颜色，最终移除隐藏效果，并处理 bold/dim 互斥。

#### Scenario: 漂亮模式隐藏标记
- **WHEN** 所有活动样式都标记 hidden 且至少有一项
- **THEN** 隐藏该段；存在 None 或非 hidden 样式时不由 all_hidden 隐藏。

#### Scenario: 主题适配
- **WHEN** 调用样式颜色适配
- **THEN** 逐字段适配前景和背景并保留 effects；当前不复制 underline_color。

证据：`crates/codegen/markdown/src/style.rs` — `MarkdownStyle`；`crates/codegen/markdown/src/style.rs` — `all_hidden`；`crates/codegen/markdown/src/colors.rs` — `adapt_style`。

### Requirement: Terminal color detection and cap
颜色探测 SHALL 缓存第一次环境判断，NO_COLOR 存在时探测为 None；可通过进程级颜色上限降低探测级别，常规适配在 TrueColor、Ansi256、Ansi16、None 间转换。

#### Scenario: 探测不可用
- **WHEN** stdout supports-color 无结果且无 NO_COLOR
- **THEN** 默认 TrueColor；受识别终端环境启发式影响，不进行实际终端查询。

#### Scenario: 环境后改
- **WHEN** 首次探测后修改环境变量
- **THEN** OnceLock 不重新探测；显式 cap 可继续改变 get_color_level。

证据：`crates/codegen/markdown/src/colors.rs` — `detect_color_level`；`crates/codegen/markdown/src/colors.rs` — `set_color_level_cap`。

### Requirement: Polarity safe syntax colors
polarity-safe 模式 SHALL 将近灰 RGB 或黑白灰 ANSI 映射为继承默认色，彩色映射为基础 ANSI 色；该进程开关在 adapt_color 中优先于一般颜色级别分支。

#### Scenario: 近灰颜色
- **WHEN** RGB 色度小于 40
- **THEN** 返回默认色；彩色依整数 hue 选基础色。

#### Scenario: 与颜色上限组合
- **WHEN** polarity-safe 开启且 cap 为 None
- **THEN** 当前早返路径不检查 cap 或 NO_COLOR，不能把 cap 当此模式的无色保证。

证据：`crates/codegen/markdown/src/colors.rs` — `polarity_safe_syntax`；`crates/codegen/markdown/src/colors.rs` — `adapt_color`。

### Requirement: Syntax lookup and highlighting
Syntect SHALL 从调用方 tmTheme 加载主题及 two-face 语法；按 UTF-8 文件扩展名或 fence token 查找语法，逐行携带状态高亮。

#### Scenario: 无效主题或语法
- **WHEN** 主题字节无效，或语法不存在
- **THEN** 无效主题构造 panic；未知语法或任一行高亮错误返回 None，调用方采用普通代码样式。

#### Scenario: 引用式 fence info
- **WHEN** info 为两个非空 ASCII 数字段加冒号路径
- **THEN** 按第三段路径扩展名查找，不检查数字顺序或范围；普通 info 按 token 查找。

证据：`crates/codegen/markdown/src/syntax.rs` — `Syntect`；`crates/codegen/markdown/src/syntax.rs` — `syntax_highlight_raw`。

### Requirement: Markdown paragraph and inline presentation
ratatui pretty 渲染 SHALL 根据 parser 记录转换列表符号、引用前缀、水平线、链接外观、数学及 HTML 实体，并按样式隐藏语法标记；raw 仍允许 force 变换和代码高亮。

#### Scenario: 列表与规则
- **WHEN** 遇到支持的 - 或 * 空格列表及 thematic break
- **THEN** pretty 使用项目符号和固定三横线；并非将所有列表 marker 形式统一重写。

#### Scenario: 图片语法
- **WHEN** 解析 Markdown Image
- **THEN** 共用 Link 的文字与目标处理，不在本包下载图片或执行 HTML DOM。

证据：`crates/codegen/markdown/src/parse.rs` — `on_start`；`crates/codegen/markdown/src/parse.rs` — `on_end`；`crates/codegen/markdown/src/render.rs` — `render_ratatui`。

### Requirement: Soft and hard break policy
soft break SHALL 默认在满足后继字符条件时用等长空格 force 替换，因此 raw 与 pretty 都受影响；可关闭 collapse_soft_breaks。table soft break 始终为空格，hard break 保留换行。

#### Scenario: LF 与 CRLF
- **WHEN** 普通 prose soft break 后不是空格、tab、> 或 |
- **THEN** LF 替换为一个空格，CRLF 对应两个空格。

#### Scenario: 关闭折行合并
- **WHEN** parser 或 streaming 设置 collapse_soft_breaks=false
- **THEN** 保留 prose 源换行；代码内部换行不由普通 soft break 分支改写。

证据：`crates/codegen/markdown/src/parse.rs` — `collapse_soft_breaks`；`crates/codegen/markdown/src/streaming.rs` — `set_collapse_soft_breaks`；`crates/codegen/markdown/src/render.rs` — `crlf`。

### Requirement: HTML entities and line break tags
pretty prose SHALL 将合法有限长度 HTML 实体转为解码文本，拒绝注入 control 字符并避开已有变换范围；inline br 标签生成换行，表格 br 进入单元格换行。

#### Scenario: 实体与代码
- **WHEN** prose 包含 &lt; 且代码包含同样文本
- **THEN** pretty prose 解码为 <；raw prose 与 code 保留字面实体。

#### Scenario: 无效实体
- **WHEN** 实体未知、缺分号或解码为 ESC 等控制字符
- **THEN** 不添加解码变换；并非完整 HTML 渲染器。

证据：`crates/codegen/markdown/src/parse.rs` — `decode_html_entity`；`crates/codegen/markdown/src/parse.rs` — `is_br_tag`；`crates/codegen/markdown/src/render.rs` — `entity_tests`。

### Requirement: Long reference compaction
ratatui pretty SHALL 按 48 显示列阈值缩写可识别的长文件或 HTTP(S) 引用并保留实际目标；代码引用要求明确 URL 或 anchored path，结构链接的描述性标签保持原文。

#### Scenario: 长自描述目标
- **WHEN** 标签是目标本身或可识别路径
- **THEN** 展示 filename 或 HTTP(S) 末个非空 path segment/host；链接目标保持完整。

#### Scenario: 表格代码路径
- **WHEN** 表格 inline code 中只有长路径且无显式链接
- **THEN** 当前不走 prose inline-code 的 compact 推断路径；不能由 prose 能力推导表格同等支持。

证据：`crates/codegen/markdown/src/parse.rs` — `reference_display_name`；`crates/codegen/markdown/src/parse.rs` — `structural_reference_display_name`；`crates/codegen/markdown/src/parse.rs` — `style_inline_code_span`。

### Requirement: Explicit hyperlink projection
HyperlinkTarget SHALL 记录输出行、显示单元列范围、URL、u32 逻辑 ID 和 Explicit/Inferred 来源；跨行或跨 span 片段保留共同逻辑 ID。

#### Scenario: 宽字符标签
- **WHEN** 标签包含 CJK 等宽字符
- **THEN** 按显示宽度而非 UTF-8 字节生成列范围。

#### Scenario: 变换后链接
- **WHEN** 链接覆盖缩写或解码文本
- **THEN** 按变换映射投影；依赖排序与合法变换边界，零宽文本可产生零宽范围。

证据：`crates/codegen/markdown/src/output.rs` — `HyperlinkTarget`；`crates/codegen/markdown/src/hyperlinks.rs` — `emit_segment_hyperlinks`。

### Requirement: Plain URL recognition
plain_url_ranges SHALL 返回 URL 类别的原文本字节范围，结合 Unicode 空白、中文标点和相邻 URL 规则拆分 prose；显式 Markdown destination 不使用该拆分规则。

#### Scenario: 查询参数中的中文分隔
- **WHEN** query 或 fragment 中含中文逗号等
- **THEN** 一般保留为数据，末尾或紧接下一个 HTTP(S) URL 时按规则截断。

#### Scenario: 跨样式裸链接
- **WHEN** 同一输出行多个 span 拼接成 URL
- **THEN** 后扫描可识别；不跨输出行连接，代码 span 也参与。

证据：`crates/codegen/markdown/src/url_scan.rs` — `plain_url_ranges`；`crates/codegen/markdown/src/url_scan.rs` — `detect_plain_urls_with_offset`。

### Requirement: Inferred hyperlink deduplication
裸 URL 后扫描 SHALL 用显示列生成 Inferred 目标；与同一行既存或新增目标的任何半开列区间重叠即抑制候选，不以 URL 相等作为前提。

#### Scenario: 重叠目标
- **WHEN** 裸 URL 与显式链接列范围重叠
- **THEN** 保留已有目标，不另加推断目标。

#### Scenario: ID 分配
- **WHEN** 新增推断目标
- **THEN** 普通 u32 递增并在完整输出排序；不承诺无间隙或溢出恢复。

证据：`crates/codegen/markdown/src/url_scan.rs` — `detect_plain_urls_with_offset`。

### Requirement: Terminal table layout
表格 SHALL 固定使用 BOX 边框和单列 padding，按单元格多行自然显示宽度、不可拆词最小宽度和可选预算分配列宽，再生成带样式行与表局部链接。

#### Scenario: 宽度收缩
- **WHEN** 预算足以容纳最小列宽但不足自然宽度
- **THEN** 按额外需求比例缩小并补足剩余列；不可拆词最小宽度超过预算时允许溢出。

#### Scenario: 折行和样式
- **WHEN** 单元格含 br、粗体、斜体、代码或链接
- **THEN** FirstFit 折行且不拆过宽词；header 加 bold，code style 可覆盖先前 cell 样式，link 最后 patch。

证据：`crates/codegen/markdown/src/parse.rs` — `format_table`；`crates/codegen/markdown/src/parse.rs` — `wrap_cell_text`；`crates/codegen/markdown/src/style.rs` — `TableBorders`。

### Requirement: Table line source offsets
表格输出 SHALL 为边框、header、分隔符和 body 行建立局部源行 offset；一个单元格展开多个视觉行时重复对应源行。

#### Scenario: body 换行
- **WHEN** 一条表格 body 行输出多行
- **THEN** 保持该 body 行对应 offset，局部链接加整表输出行偏移。

#### Scenario: 公开边框配置
- **WHEN** 调用方构造 TableBorders::ASCII 或 DOUBLE
- **THEN** 类型可用，但当前 MarkdownParser 的 format_table 不提供选择这些边框的参数。

证据：`crates/codegen/markdown/src/parse.rs` — `format_table`；`crates/codegen/markdown/src/style.rs` — `TableBorders`。

### Requirement: Closed fenced code metadata
ratatui 输出 SHALL 为结构上闭合的 fenced code 保留完整 info、去容器标记后的 body、正文源字节范围及 pre-wrap 输出行范围；不将普通缩进代码当 fenced metadata。

#### Scenario: 围栏未闭合
- **WHEN** 只有 EOF 合成 CodeBlock end
- **THEN** 不产出闭合 CodeBlockSpan；空闭合 body 使用空范围。

#### Scenario: 容器与规范化
- **WHEN** fence 在 quote/list 内或使用 CRLF
- **THEN** body 为 parser clean 内容，source range 属于 parser 输入，可包含容器前缀且高层已执行 LaTeX 规范化。

证据：`crates/codegen/markdown/src/output.rs` — `CodeBlockSpan`；`crates/codegen/markdown/src/output.rs` — `build_code_block_spans`；`crates/codegen/markdown/src/parse.rs` — `CodeBlockMeta`。

### Requirement: ANSI and ratatui rendering boundaries
ANSI 渲染 SHALL 输出样式转义及 byte SourceMap；普通文本只执行等长 force 变换，pretty table、display math 与 Mermaid 使用 plain replacement 行。ratatui 执行普通 pretty 变换并保留结构化样式。

#### Scenario: 行内数学或 entity
- **WHEN** 比较两种 pretty 输出
- **THEN** 不承诺去 ANSI 后与 ratatui 文本等价，因为 ANSI 普通文本没有 apply 非-force 变换。

#### Scenario: 代码替换
- **WHEN** 使用 Syntect 高亮代码
- **THEN** ANSI 去语法背景并输出 reset；ratatui 用 MarkdownStyle.code_background 设置代码行背景。

证据：`crates/codegen/markdown/src/render.rs` — `render_ansi`；`crates/codegen/markdown/src/render.rs` — `render_ratatui`。

### Requirement: Source map partial lookup
SourceMap SHALL 按等长段记录 rendered 到 source 字节映射，支持偏移追加、截断、清空和段检查；to_source 返回所有相交部分的最小起点到最大终点。

#### Scenario: 不连续源片段
- **WHEN** 查询范围与多个不连续 source 段相交
- **THEN** 当前返回包围区间而非 None；不验证整段查询都被覆盖。

#### Scenario: 无匹配
- **WHEN** 查询不与任何段相交
- **THEN** 返回 None；ANSI table/Mermaid 插入行不添加对应段，不提供无损完整回源保证。

证据：`crates/codegen/markdown/src/source_map.rs` — `to_source`；`crates/codegen/markdown/src/render.rs` — `render_ansi`。

### Requirement: Reusable parser state boundaries
MarkdownBuffers SHALL 保留中间 Vec 容量供复用，parser 清理旧数据后填充；公开 ParsedMarkdown 构造器不验证调用方传入范围与排序。

#### Scenario: 重复消费 parsed 输出
- **WHEN** 同一 ParsedMarkdown 多次 render_ratatui
- **THEN** code_blocks metadata 经 mem::take 消费，不能推导重复输出 metadata 幂等。

#### Scenario: 不合法变换
- **WHEN** 调用方手工提供长度或范围不合法的 force 变换
- **THEN** 存在 assertion 或 slice panic；这不是可容错反序列化接口。

证据：`crates/codegen/markdown/src/buffers.rs` — `MarkdownBuffers`；`crates/codegen/markdown/src/parse.rs` — `ParsedMarkdown`；`crates/codegen/markdown/src/render.rs` — `mem::take`。

### Requirement: Streaming ingestion and finalization
StreamingMarkdownRenderer SHALL 将 push 的规范化前缀存入 source；push 不渲染，render 重算尾部，push_and_render 合并两步。finish 刷新暂存并完整重渲染，finish_into_output 返回完成结果。

#### Scenario: 只取得现有输出
- **WHEN** 调用 into_output
- **THEN** 不隐式 finish，不刷新尚未判定的分隔符尾部。

#### Scenario: 流结束
- **WHEN** 调用 finish
- **THEN** 从 link ID 0 重建输出，扫描并排序裸 URL，将全部源与输出冻结，释放高亮缓存；不消费 renderer。

证据：`crates/codegen/markdown/src/streaming.rs` — `push_and_render`；`crates/codegen/markdown/src/streaming.rs` — `finish_into_output`；`crates/codegen/markdown/src/streaming.rs` — `into_output`。

### Requirement: Streaming checkpoints and tail rebuilding
流式渲染 SHALL 在 parser 提供的顶层边界冻结输出前缀，仅重算未冻结源尾部；Checkpoint 记录源字节数、输出行数和八种块类型。

#### Scenario: 容器内部
- **WHEN** 代码等块仍在列表、引用或表格中
- **THEN** 不在内部边界冻结；顶层 paragraph/heading 等需要后续空行条件，code 与 thematic break 有独立规则。

#### Scenario: 源保留
- **WHEN** 前缀已经冻结
- **THEN** source 仍保留全部规范化内容；并非丢弃历史或保证整个入口严格线性复杂度。

证据：`crates/codegen/markdown/src/checkpoint.rs` — `CheckpointKind`；`crates/codegen/markdown/src/parse.rs` — `has_blank_line_after`；`crates/codegen/markdown/src/streaming.rs` — `rerender_tail`。

### Requirement: Streaming metadata offsets
尾部渲染 SHALL 截回冻结输出，保留冻结链接及闭合 code span，再为新链接和 code span 加输出行或源字节偏移。

#### Scenario: 增量行映射
- **WHEN** 已有冻结前缀后追加并渲染尾部
- **THEN** 当前直接追加 tail-relative line_source_map，不加源行偏移；不能将中间态行号当整篇绝对源行。

#### Scenario: 完成重建
- **WHEN** 调用 finish
- **THEN** 从完整源重建映射和 ID；中间态 metadata 等价不能由最终纯文本等价测试推出。

证据：`crates/codegen/markdown/src/streaming.rs` — `rerender_tail`；`crates/codegen/markdown/src/streaming.rs` — `finish`。

### Requirement: Streaming settings and clone
streaming SHALL 在 set_style 时无条件清空输出、冻结状态和高亮缓存；pretty、宽度或 soft-break 值变化时也使这些状态失效，等待下一次 render。

#### Scenario: clear
- **WHEN** 调用 clear
- **THEN** 清 source/output/frozen/normalizer/cache，并将宽度设 None；保留 style、pretty、soft-break 配置。

#### Scenario: clone
- **WHEN** 克隆已有 renderer
- **THEN** 复制规范化 source、normalizer 状态及配置后以 None Syntect 渲染，不保证保留原高亮。

证据：`crates/codegen/markdown/src/streaming.rs` — `set_style`；`crates/codegen/markdown/src/streaming.rs` — `clear`；`crates/codegen/markdown/src/streaming.rs` — `fn clone`。

### Requirement: Open fence incremental highlighting
流式高亮 SHALL 对相同 info、源起点和 append-only 已提交前缀复用 open fence 的逐行状态，未换行尾部在克隆状态上试算；前缀或身份变化时重建。

#### Scenario: 主题更换
- **WHEN** 更换 Syntect 主题
- **THEN** 调用方需先通过 set_style 等重置缓存；cache key 不含主题身份。

#### Scenario: 长代码持续输入
- **WHEN** 追加 open fenced code 行
- **THEN** 避免重新 Syntect parse 已提交行，但仍扫描前缀和 clone 输出，不保证全部处理 O(N)。

证据：`crates/codegen/markdown/src/open_fence_highlighter.rs` — `OpenFenceHighlighter`；`crates/codegen/markdown/src/streaming.rs` — `set_style`。

### Requirement: Closed fence memoization budget
closed fence 高亮 SHALL 按 info 与完整 body 记忆结果，累计 body key 预算为 256 KiB，超限先清理再插入当前条目。

#### Scenario: 单条超预算
- **WHEN** 一个 body 大于 256 KiB
- **THEN** 当前仍插入该条，预算不是总分配硬上限；info 和输出 span 不计入此预算。

#### Scenario: 命中缓存
- **WHEN** 同 key 重渲染
- **THEN** clone 已缓存 span 内容；不是零复制，也不识别 Syntect 对象变更。

证据：`crates/codegen/markdown/src/open_fence_highlighter.rs` — `closed`。

### Requirement: LaTeX delimiter normalization
LaTeX 规范化 SHALL 将代码识别范围之外的 paren、bracket、equation/equation* 分隔形式归一为 dollar 形式，并处理 display 内部换行，支持分 chunk 暂存歧义尾部。

#### Scenario: 分块分隔符
- **WHEN** 一个 delimiter 跨 push 边界
- **THEN** 保留未确定字节至后续输入或 finish；source 暂时不包含这些字节。

#### Scenario: 结束与重置
- **WHEN** 调用 normalizer.finish 或 reset
- **THEN** finish 仍执行最终规范化而非一律字面 flush；reset 才恢复初始状态并丢弃 pending。

证据：`crates/codegen/markdown/src/latex_delimiters.rs` — `LatexDelimiterNormalizer`；`crates/codegen/markdown/src/latex_delimiters.rs` — `emit_display_span`。

### Requirement: LaTeX normalizer current code boundaries
当前 normalizer SHALL 按其单行 inline-code 和 LF fenced-close 识别规则工作；4096 数学长度检查不构成所有 pending 路径的全局上限。

#### Scenario: CRLF 关闭围栏
- **WHEN** CRLF fence close 后再出现 paren 数学
- **THEN** 当前 close 未被识别，后续数学保持字面；多行 backtick span 内数学则可能被转换。

#### Scenario: 长歧义尾部
- **WHEN** 输入 20000 backtick 或 display opener 后 LF 与 20000 空格
- **THEN** push 可暂存约 20000 字节；已用真实模块探针确认，不声称生产资源耗尽已复现。

证据：`crates/codegen/markdown/src/latex_delimiters.rs` — `scan_fence_close`；`crates/codegen/markdown/src/latex_delimiters.rs` — `find_display_close`。

### Requirement: LaTeX conversion limits and fallback
数学转换 SHALL 以 UTF-8 字节 4096 为单次 source 上限；超限返回 None，parser 使用 code 风格内容回退。inline 将多行扁平连接，display 保留二维行布局。

#### Scenario: 超长数学
- **WHEN** 数学 body 超过上限
- **THEN** 保留 body 的 fallback 内容，不承诺一般 TeX 解释。

#### Scenario: 深度与未知命令
- **WHEN** 遇到深花括号或未知控制词
- **THEN** 花括号分支达到 32 层后输出原 group；未知命令保留裸名称，32 不是所有命令/环境递归的统一限深。

证据：`crates/codegen/markdown/src/latex/mod.rs` — `MAX_MATH_SOURCE_LEN`；`crates/codegen/markdown/src/latex/commands.rs` — `MAX_DEPTH`；`crates/codegen/markdown/src/parse.rs` — `InlineMath`。

### Requirement: LaTeX symbols scripts and alphabets
LaTeX 近似转换 SHALL 提供符号查表、上下标、Unicode 数学字母和 text-family；仅全部字符可映射且不满足 wordlike 启发式时使用 Unicode script。

#### Scenario: 文本式下标
- **WHEN** source 含 text-family 或结果有连续三个 ASCII 字母
- **THEN** 保留 ^/_ 形式，多字符按规则加括号。

#### Scenario: 不支持字形
- **WHEN** 数学 alphabet 中字符没有对应 Unicode 映射
- **THEN** 保留原字符；不是渲染自定义字体。

证据：`crates/codegen/markdown/src/latex/symbols.rs` — `superscript`；`crates/codegen/markdown/src/latex/commands.rs` — `wordlike`。

### Requirement: LaTeX fractions roots and accents
数学近似 SHALL 对固定 vulgar fraction 使用单字符，其余分数用带必要括号的斜杠表达；支持根号索引、binomial、boxes、combining accents 和有限 not 关系。

#### Scenario: 装饰无法映射
- **WHEN** overset/underset 的装饰不能全部转为 script
- **THEN** 当前省略装饰而保留基底；不声称无损公式呈现。

#### Scenario: 多字符重音
- **WHEN** accent 作用于多字符结果
- **THEN** 对每个非空白字符追加 combining mark，不进行一般二维重音排版。

证据：`crates/codegen/markdown/src/latex/commands.rs` — `overset`；`crates/codegen/markdown/src/latex/commands.rs` — `fraction`。

### Requirement: LaTeX environment layout
数学环境 SHALL 支持矩阵、cases 和 aligned 等行列近似，按显示宽度排版 MathBox；环境名可去尾星号，array/alignat 消费列配置。

#### Scenario: 缺少结束环境
- **WHEN** 没有匹配 end
- **THEN** 消费到 EOF；未知环境按行拼接近似处理。

#### Scenario: 嵌套二维环境
- **WHEN** 矩阵 cell 内再含矩阵
- **THEN** cell 先 flat 渲染，内部二维形状不保留；vmatrix 与 Vmatrix 当前都用单竖线。

证据：`crates/codegen/markdown/src/latex/environments.rs` — `render_environment`；`crates/codegen/markdown/src/latex/math_box.rs` — `MathBox`。

### Requirement: Markdown playground feature
playground feature SHALL 启用 md-table-test 与 md-mermaid-test，以及 crossterm 和 ratatui-textarea 可选依赖；两个工具使用共享 Tokyo Night 主题和真实 Syntect。

#### Scenario: 默认构建
- **WHEN** 没有启用 playground
- **THEN** 两个 required-features binary 不开放。

#### Scenario: 表格比较
- **WHEN** 在 md-table-test 编辑文本或调整宽度
- **THEN** 并排显示 full 与逐字符 streaming 的当前 view，只比较纯文本行，不比较样式、链接、映射且 streaming 不调用 finish。

证据：`crates/codegen/markdown/Cargo.toml` — `playground`；`crates/codegen/markdown/bin/md_table_test.rs` — `detect_mismatch`；`crates/codegen/markdown/bin/playground_common.rs` — `get_syntect`。

### Requirement: Markdown playground interaction
playground SHALL 在 alternate screen/raw mode 下读取按键；Ctrl-Q 全局退出，编辑态 Esc/Tab 离焦，其他按键转交 textarea 并实时重渲染。

#### Scenario: 表格查看态
- **WHEN** 按 Space/Enter 或 h/l/H/L
- **THEN** 重新编辑或调整宽度 1/5；最小宽度按原文 pipe 数估计并至少 10，不是 parser 列数。

#### Scenario: Mermaid 查看态
- **WHEN** 按 Tab、n 或 h/l
- **THEN** 进入编辑、轮换 13 个 sample 或调整宽度 2；初始 sample/width 从 MERMAID_SAMPLE/MERMAID_WIDTH 读取，默认 0/70。

证据：`crates/codegen/markdown/bin/md_table_test.rs` — `fn main`；`crates/codegen/markdown/bin/md_table_test.rs` — `min_render_width`；`crates/codegen/markdown/bin/md_mermaid_test.rs` — `fn main`。

### Requirement: Markdown performance fixtures
Criterion bench SHALL 提供常规、链接、数学、裸 URL、open YAML 和 list 内 closed fence 的 full/streaming 测量 fixture，主题在计时循环外构造。

#### Scenario: 流式分块
- **WHEN** 运行一般 streaming benchmark
- **THEN** 按含尾空白 token 分块；只有 plain-URL benchmark 明确 finish，不将其余结果当完成态。

#### Scenario: 性能注释
- **WHEN** fixture 注释写 O(N) 或历史耗时
- **THEN** 没有自动阈值断言；本次仅阅读而未运行性能测量，不能据 fixture 得出性能保证。

证据：`crates/codegen/markdown/benches/bench.rs` — `criterion_group!`。
