# markdown 逐包核查

包路径：`crates/codegen/markdown`。全部所属 Rust 模块和 Cargo.toml 已阅读；测试执行边界见本文件末尾。

## 模块与开关

- `crates/codegen/markdown/Cargo.toml`
- `crates/codegen/markdown/assets/tokyo-night.tmTheme`
- `crates/codegen/markdown/benches/bench.rs`
- `crates/codegen/markdown/bin/md_mermaid_test.rs`
- `crates/codegen/markdown/bin/md_table_test.rs`
- `crates/codegen/markdown/bin/playground_common.rs`
- `crates/codegen/markdown/src/buffers.rs`
- `crates/codegen/markdown/src/checkpoint.rs`
- `crates/codegen/markdown/src/colors.rs`
- `crates/codegen/markdown/src/hyperlinks.rs`
- `crates/codegen/markdown/src/latex/commands.rs`
- `crates/codegen/markdown/src/latex/cursor.rs`
- `crates/codegen/markdown/src/latex/environments.rs`
- `crates/codegen/markdown/src/latex/math_box.rs`
- `crates/codegen/markdown/src/latex/mod.rs`
- `crates/codegen/markdown/src/latex/symbols.rs`
- `crates/codegen/markdown/src/latex/tests.rs`
- `crates/codegen/markdown/src/latex_delimiters.rs`
- `crates/codegen/markdown/src/lib.rs`
- `crates/codegen/markdown/src/mermaid.rs`
- `crates/codegen/markdown/src/open_fence_highlighter.rs`
- `crates/codegen/markdown/src/output.rs`
- `crates/codegen/markdown/src/parse.rs`
- `crates/codegen/markdown/src/parser_policy.rs`
- `crates/codegen/markdown/src/render.rs`
- `crates/codegen/markdown/src/source_map.rs`
- `crates/codegen/markdown/src/streaming.rs`
- `crates/codegen/markdown/src/style.rs`
- `crates/codegen/markdown/src/syntax.rs`
- `crates/codegen/markdown/src/url_scan.rs`

Cargo feature：`{"default": [], "playground": ["dep:crossterm", "dep:ratatui-textarea"]}`。

## 功能与规范映射

- [Markdown rendering entrypoints](../specs/terminal-markdown/spec.md#requirement-markdown-rendering-entrypoints)：Markdown SHALL 提供 ANSI 字符串与 SourceMap、ratatui 完整输出与 checkpoint、简化 lines 与 line map，以及可复用 MarkdownBuffers 和可选表格宽度的入口；一次性入口先规范化 LaTeX 分隔符，坐标属于规范化后的输入。
- [Markdown parser policy](../specs/terminal-markdown/spec.md#requirement-markdown-parser-policy)：MarkdownParser SHALL 使用统一的 GFM、表格、任务列表、数学和删除线策略；仅双波浪线产生删除线事件，单波浪线保持字面文本。
- [Markdown style composition](../specs/terminal-markdown/spec.md#requirement-markdown-style-composition)：MarkdownStyle SHALL 分别配置六级 heading 内外、强调、代码、列表、引用、任务、链接、表格与数学样式；Default 为无修饰样式。组合按次序覆盖颜色，最终移除隐藏效果，并处理 bold/dim 互斥。
- [Terminal color detection and cap](../specs/terminal-markdown/spec.md#requirement-terminal-color-detection-and-cap)：颜色探测 SHALL 缓存第一次环境判断，NO_COLOR 存在时探测为 None；可通过进程级颜色上限降低探测级别，常规适配在 TrueColor、Ansi256、Ansi16、None 间转换。
- [Polarity safe syntax colors](../specs/terminal-markdown/spec.md#requirement-polarity-safe-syntax-colors)：polarity-safe 模式 SHALL 将近灰 RGB 或黑白灰 ANSI 映射为继承默认色，彩色映射为基础 ANSI 色；该进程开关在 adapt_color 中优先于一般颜色级别分支。
- [Syntax lookup and highlighting](../specs/terminal-markdown/spec.md#requirement-syntax-lookup-and-highlighting)：Syntect SHALL 从调用方 tmTheme 加载主题及 two-face 语法；按 UTF-8 文件扩展名或 fence token 查找语法，逐行携带状态高亮。
- [Markdown paragraph and inline presentation](../specs/terminal-markdown/spec.md#requirement-markdown-paragraph-and-inline-presentation)：ratatui pretty 渲染 SHALL 根据 parser 记录转换列表符号、引用前缀、水平线、链接外观、数学及 HTML 实体，并按样式隐藏语法标记；raw 仍允许 force 变换和代码高亮。
- [Soft and hard break policy](../specs/terminal-markdown/spec.md#requirement-soft-and-hard-break-policy)：soft break SHALL 默认在满足后继字符条件时用等长空格 force 替换，因此 raw 与 pretty 都受影响；可关闭 collapse_soft_breaks。table soft break 始终为空格，hard break 保留换行。
- [HTML entities and line break tags](../specs/terminal-markdown/spec.md#requirement-html-entities-and-line-break-tags)：pretty prose SHALL 将合法有限长度 HTML 实体转为解码文本，拒绝注入 control 字符并避开已有变换范围；inline br 标签生成换行，表格 br 进入单元格换行。
- [Long reference compaction](../specs/terminal-markdown/spec.md#requirement-long-reference-compaction)：ratatui pretty SHALL 按 48 显示列阈值缩写可识别的长文件或 HTTP(S) 引用并保留实际目标；代码引用要求明确 URL 或 anchored path，结构链接的描述性标签保持原文。
- [Explicit hyperlink projection](../specs/terminal-markdown/spec.md#requirement-explicit-hyperlink-projection)：HyperlinkTarget SHALL 记录输出行、显示单元列范围、URL、u32 逻辑 ID 和 Explicit/Inferred 来源；跨行或跨 span 片段保留共同逻辑 ID。
- [Plain URL recognition](../specs/terminal-markdown/spec.md#requirement-plain-url-recognition)：plain_url_ranges SHALL 返回 URL 类别的原文本字节范围，结合 Unicode 空白、中文标点和相邻 URL 规则拆分 prose；显式 Markdown destination 不使用该拆分规则。
- [Inferred hyperlink deduplication](../specs/terminal-markdown/spec.md#requirement-inferred-hyperlink-deduplication)：裸 URL 后扫描 SHALL 用显示列生成 Inferred 目标；与同一行既存或新增目标的任何半开列区间重叠即抑制候选，不以 URL 相等作为前提。
- [Terminal table layout](../specs/terminal-markdown/spec.md#requirement-terminal-table-layout)：表格 SHALL 固定使用 BOX 边框和单列 padding，按单元格多行自然显示宽度、不可拆词最小宽度和可选预算分配列宽，再生成带样式行与表局部链接。
- [Table line source offsets](../specs/terminal-markdown/spec.md#requirement-table-line-source-offsets)：表格输出 SHALL 为边框、header、分隔符和 body 行建立局部源行 offset；一个单元格展开多个视觉行时重复对应源行。
- [Closed fenced code metadata](../specs/terminal-markdown/spec.md#requirement-closed-fenced-code-metadata)：ratatui 输出 SHALL 为结构上闭合的 fenced code 保留完整 info、去容器标记后的 body、正文源字节范围及 pre-wrap 输出行范围；不将普通缩进代码当 fenced metadata。
- [ANSI and ratatui rendering boundaries](../specs/terminal-markdown/spec.md#requirement-ansi-and-ratatui-rendering-boundaries)：ANSI 渲染 SHALL 输出样式转义及 byte SourceMap；普通文本只执行等长 force 变换，pretty table、display math 与 Mermaid 使用 plain replacement 行。ratatui 执行普通 pretty 变换并保留结构化样式。
- [Source map partial lookup](../specs/terminal-markdown/spec.md#requirement-source-map-partial-lookup)：SourceMap SHALL 按等长段记录 rendered 到 source 字节映射，支持偏移追加、截断、清空和段检查；to_source 返回所有相交部分的最小起点到最大终点。
- [Reusable parser state boundaries](../specs/terminal-markdown/spec.md#requirement-reusable-parser-state-boundaries)：MarkdownBuffers SHALL 保留中间 Vec 容量供复用，parser 清理旧数据后填充；公开 ParsedMarkdown 构造器不验证调用方传入范围与排序。
- [Streaming ingestion and finalization](../specs/terminal-markdown/spec.md#requirement-streaming-ingestion-and-finalization)：StreamingMarkdownRenderer SHALL 将 push 的规范化前缀存入 source；push 不渲染，render 重算尾部，push_and_render 合并两步。finish 刷新暂存并完整重渲染，finish_into_output 返回完成结果。
- [Streaming checkpoints and tail rebuilding](../specs/terminal-markdown/spec.md#requirement-streaming-checkpoints-and-tail-rebuilding)：流式渲染 SHALL 在 parser 提供的顶层边界冻结输出前缀，仅重算未冻结源尾部；Checkpoint 记录源字节数、输出行数和八种块类型。
- [Streaming metadata offsets](../specs/terminal-markdown/spec.md#requirement-streaming-metadata-offsets)：尾部渲染 SHALL 截回冻结输出，保留冻结链接及闭合 code span，再为新链接和 code span 加输出行或源字节偏移。
- [Streaming settings and clone](../specs/terminal-markdown/spec.md#requirement-streaming-settings-and-clone)：streaming SHALL 在 set_style 时无条件清空输出、冻结状态和高亮缓存；pretty、宽度或 soft-break 值变化时也使这些状态失效，等待下一次 render。
- [Open fence incremental highlighting](../specs/terminal-markdown/spec.md#requirement-open-fence-incremental-highlighting)：流式高亮 SHALL 对相同 info、源起点和 append-only 已提交前缀复用 open fence 的逐行状态，未换行尾部在克隆状态上试算；前缀或身份变化时重建。
- [Closed fence memoization budget](../specs/terminal-markdown/spec.md#requirement-closed-fence-memoization-budget)：closed fence 高亮 SHALL 按 info 与完整 body 记忆结果，累计 body key 预算为 256 KiB，超限先清理再插入当前条目。
- [LaTeX delimiter normalization](../specs/terminal-markdown/spec.md#requirement-latex-delimiter-normalization)：LaTeX 规范化 SHALL 将代码识别范围之外的 paren、bracket、equation/equation* 分隔形式归一为 dollar 形式，并处理 display 内部换行，支持分 chunk 暂存歧义尾部。
- [LaTeX normalizer current code boundaries](../specs/terminal-markdown/spec.md#requirement-latex-normalizer-current-code-boundaries)：当前 normalizer SHALL 按其单行 inline-code 和 LF fenced-close 识别规则工作；4096 数学长度检查不构成所有 pending 路径的全局上限。
- [LaTeX conversion limits and fallback](../specs/terminal-markdown/spec.md#requirement-latex-conversion-limits-and-fallback)：数学转换 SHALL 以 UTF-8 字节 4096 为单次 source 上限；超限返回 None，parser 使用 code 风格内容回退。inline 将多行扁平连接，display 保留二维行布局。
- [LaTeX symbols scripts and alphabets](../specs/terminal-markdown/spec.md#requirement-latex-symbols-scripts-and-alphabets)：LaTeX 近似转换 SHALL 提供符号查表、上下标、Unicode 数学字母和 text-family；仅全部字符可映射且不满足 wordlike 启发式时使用 Unicode script。
- [LaTeX fractions roots and accents](../specs/terminal-markdown/spec.md#requirement-latex-fractions-roots-and-accents)：数学近似 SHALL 对固定 vulgar fraction 使用单字符，其余分数用带必要括号的斜杠表达；支持根号索引、binomial、boxes、combining accents 和有限 not 关系。
- [LaTeX environment layout](../specs/terminal-markdown/spec.md#requirement-latex-environment-layout)：数学环境 SHALL 支持矩阵、cases 和 aligned 等行列近似，按显示宽度排版 MathBox；环境名可去尾星号，array/alignat 消费列配置。
- [Terminal Mermaid dispatch](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-dispatch)：markdown 内置 Mermaid SHALL 在 mermaid fence 中尝试 flowchart/graph、state、class、ER 和 sequence 五类终端文字渲染；空源返回 None，不支持或布局超限使用原文框回退。
- [Terminal Mermaid flow syntax](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-flow-syntax)：flow parser SHALL 支持方向、带形状 label 的节点、链式或成组端点、不同线型和箭头，以及带标签边；解析为受限文字图而非完整 Mermaid 语言。
- [Terminal Mermaid label cleaning](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-label-cleaning)：图 label SHALL 清理有限 HTML 格式标签、引号和 Markdown 标记，再单次解码有限 named/numeric entity；普通 label 按显示宽度折行和省略。
- [Terminal Mermaid subgraphs](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-subgraphs)：subgraph SHALL 记录嵌套作用域和首次归属，限制 24 个组及深度 6；将 group proxy 和跨组边提升至共同祖先作用域并绘制组框。
- [Terminal Mermaid state diagrams](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-state-diagrams)：state parser SHALL 支持 state 声明、描述、转换标签及初始/终止伪节点，将 composite state 扁平为图；部分 metadata 和 note 被忽略。
- [Terminal Mermaid class and ER diagrams](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-class-and-er-diagrams)：class/ER SHALL 通过分区节点显示名称、成员或属性及关系；class 支持 annotation、generic 显示及有限关系符，ER 将 cardinality 转为文字标签。
- [Terminal Mermaid graph layout](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-graph-layout)：终端图布局 SHALL 以去灰色回边的 DFS 构建分层，保留原循环边绘制侧通道；通过最多 8 轮重心排序与 10 轮位置松弛布局，支持 TD/LR 及最终 BT/RL 翻转。
- [Terminal Mermaid canvas bounds](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-canvas-bounds)：正常图和 sequence 布局 SHALL 在最终 Canvas 分配前检查宽度及单画布 2^21 格限制；宽字符用 continuation 哨兵表示并在输出时跳过。
- [Terminal Mermaid sequence events](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-sequence-events)：sequence SHALL 支持 participant/actor、隐式参与者、八类消息操作符、自消息、单行 note、autonumber 和有限控制块分隔线；限制 128 个参与者和 512 个事件。
- [Terminal Mermaid sequence layout](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-sequence-layout)：sequence SHALL 按参与者间距需求布置消息与 note，顶部和底部重复参与者框，并绘制 lifeline；note over 最多使用前两个参与者。
- [Terminal Mermaid fallback output](../specs/diagram-rendering/spec.md#requirement-terminal-mermaid-fallback-output)：fallback SHALL 把原源行放入文字框，仅宽度失败时附打开图片的文字提示；unsupported 或 cell 超限不附该提示。
- [Markdown playground feature](../specs/terminal-markdown/spec.md#requirement-markdown-playground-feature)：playground feature SHALL 启用 md-table-test 与 md-mermaid-test，以及 crossterm 和 ratatui-textarea 可选依赖；两个工具使用共享 Tokyo Night 主题和真实 Syntect。
- [Markdown playground interaction](../specs/terminal-markdown/spec.md#requirement-markdown-playground-interaction)：playground SHALL 在 alternate screen/raw mode 下读取按键；Ctrl-Q 全局退出，编辑态 Esc/Tab 离焦，其他按键转交 textarea 并实时重渲染。
- [Markdown performance fixtures](../specs/terminal-markdown/spec.md#requirement-markdown-performance-fixtures)：Criterion bench SHALL 提供常规、链接、数学、裸 URL、open YAML 和 list 内 closed fence 的 full/streaming 测量 fixture，主题在计时循环外构造。

## 边界

- 返回样式行、行源映射、超链接与闭合 fenced code metadata；裸 URL 后扫描并按行列排序。
- 同样先规范化 LaTeX，但不执行 ratatui 的裸 URL metadata 后扫描；不产生 OSC 8。
- 不作为删除线；~~word~~ 仍使用删除线样式。
- 解析其收到的文本并清理复用 buffers；不自动执行高层 LaTeX 规范化。
- 隐藏该段；存在 None 或非 hidden 样式时不由 all_hidden 隐藏。
- 逐字段适配前景和背景并保留 effects；当前不复制 underline_color。
- 默认 TrueColor；受识别终端环境启发式影响，不进行实际终端查询。
- OnceLock 不重新探测；显式 cap 可继续改变 get_color_level。
- 返回默认色；彩色依整数 hue 选基础色。
- 当前早返路径不检查 cap 或 NO_COLOR，不能把 cap 当此模式的无色保证。
- 无效主题构造 panic；未知语法或任一行高亮错误返回 None，调用方采用普通代码样式。
- 按第三段路径扩展名查找，不检查数字顺序或范围；普通 info 按 token 查找。
- pretty 使用项目符号和固定三横线；并非将所有列表 marker 形式统一重写。
- 共用 Link 的文字与目标处理，不在本包下载图片或执行 HTML DOM。
- LF 替换为一个空格，CRLF 对应两个空格。
- 保留 prose 源换行；代码内部换行不由普通 soft break 分支改写。
- pretty prose 解码为 <；raw prose 与 code 保留字面实体。
- 不添加解码变换；并非完整 HTML 渲染器。
- 展示 filename 或 HTTP(S) 末个非空 path segment/host；链接目标保持完整。
- 当前不走 prose inline-code 的 compact 推断路径；不能由 prose 能力推导表格同等支持。
- 按显示宽度而非 UTF-8 字节生成列范围。
- 按变换映射投影；依赖排序与合法变换边界，零宽文本可产生零宽范围。
- 一般保留为数据，末尾或紧接下一个 HTTP(S) URL 时按规则截断。
- 后扫描可识别；不跨输出行连接，代码 span 也参与。
- 保留已有目标，不另加推断目标。
- 普通 u32 递增并在完整输出排序；不承诺无间隙或溢出恢复。
- 按额外需求比例缩小并补足剩余列；不可拆词最小宽度超过预算时允许溢出。
- FirstFit 折行且不拆过宽词；header 加 bold，code style 可覆盖先前 cell 样式，link 最后 patch。
- 保持该 body 行对应 offset，局部链接加整表输出行偏移。
- 类型可用，但当前 MarkdownParser 的 format_table 不提供选择这些边框的参数。
- 不产出闭合 CodeBlockSpan；空闭合 body 使用空范围。
- body 为 parser clean 内容，source range 属于 parser 输入，可包含容器前缀且高层已执行 LaTeX 规范化。
- 不承诺去 ANSI 后与 ratatui 文本等价，因为 ANSI 普通文本没有 apply 非-force 变换。
- ANSI 去语法背景并输出 reset；ratatui 用 MarkdownStyle.code_background 设置代码行背景。
- 当前返回包围区间而非 None；不验证整段查询都被覆盖。
- 返回 None；ANSI table/Mermaid 插入行不添加对应段，不提供无损完整回源保证。
- code_blocks metadata 经 mem::take 消费，不能推导重复输出 metadata 幂等。
- 存在 assertion 或 slice panic；这不是可容错反序列化接口。
- 不隐式 finish，不刷新尚未判定的分隔符尾部。
- 从 link ID 0 重建输出，扫描并排序裸 URL，将全部源与输出冻结，释放高亮缓存；不消费 renderer。
- 不在内部边界冻结；顶层 paragraph/heading 等需要后续空行条件，code 与 thematic break 有独立规则。
- source 仍保留全部规范化内容；并非丢弃历史或保证整个入口严格线性复杂度。
- 当前直接追加 tail-relative line_source_map，不加源行偏移；不能将中间态行号当整篇绝对源行。
- 从完整源重建映射和 ID；中间态 metadata 等价不能由最终纯文本等价测试推出。
- 清 source/output/frozen/normalizer/cache，并将宽度设 None；保留 style、pretty、soft-break 配置。
- 复制规范化 source、normalizer 状态及配置后以 None Syntect 渲染，不保证保留原高亮。
- 调用方需先通过 set_style 等重置缓存；cache key 不含主题身份。
- 避免重新 Syntect parse 已提交行，但仍扫描前缀和 clone 输出，不保证全部处理 O(N)。
- 当前仍插入该条，预算不是总分配硬上限；info 和输出 span 不计入此预算。
- clone 已缓存 span 内容；不是零复制，也不识别 Syntect 对象变更。
- 保留未确定字节至后续输入或 finish；source 暂时不包含这些字节。
- finish 仍执行最终规范化而非一律字面 flush；reset 才恢复初始状态并丢弃 pending。
- 当前 close 未被识别，后续数学保持字面；多行 backtick span 内数学则可能被转换。
- push 可暂存约 20000 字节；已用真实模块探针确认，不声称生产资源耗尽已复现。
- 保留 body 的 fallback 内容，不承诺一般 TeX 解释。
- 花括号分支达到 32 层后输出原 group；未知命令保留裸名称，32 不是所有命令/环境递归的统一限深。
- 保留 ^/_ 形式，多字符按规则加括号。
- 保留原字符；不是渲染自定义字体。
- 当前省略装饰而保留基底；不声称无损公式呈现。
- 对每个非空白字符追加 combining mark，不进行一般二维重音排版。
- 消费到 EOF；未知环境按行拼接近似处理。
- cell 先 flat 渲染，内部二维形状不保留；vmatrix 与 Vmatrix 当前都用单竖线。
- 尝试该文字渲染路径；此条件独立于最终 closed code metadata 检测。
- 同一预算传给 Mermaid 布局；本路径不生成图片。
- 解析路径回退；源 statement 字符串在计数检查之前收集，不能推导总输入内存上限。
- 元数据保留该形状，但当前 draw_box 与 Round 一样画圆角框。
- 优先在 _ - . / 后断开，否则逐字符，最多显示 4 行；构建过程可先产生更多行。
- statement splitter 可先拆开，不能由直接 label helper 测试声称完整 parser 接受该形式。
- 可以移除空组。
- 组内方向被忽略；子画布布局不传外层宽度，仅外层最终比较预算。
- 记录 Diamond 元数据，但最终终端绘制仍受通用 draw_box 限制。
- 不建立与 flow subgraph 同等的嵌套可视化语义。
- 显示有限成员并附省略号。
- 当前按空白切 token 的关系 parser 不能保证接受；现有 contains 文本测试可能仅看到 fallback。
- 使用侧轨道；不承诺全局最优或同构规范布局。
- 停止写入而不另寻位置；多条 self-loop 可共用路径，不能保证所有 label 和端点同时完整显示。
- 返回 oversize 并回退，不为该最终画布分配。
- 单画布限制不约束所有字符串、子画布同时存活或 fallback 的总体内存；字符绘制不等同 grapheme-aware 布局。
- 当前只开启 bool，自 1 计数，忽略参数。
- activation 不执行；rect/box 只影响内部块栈而不画区域，栈没有独立深度上限。
- 不支持一般多行 note 块；可用 note 采用固定三行框。
- 完整文本宽度影响间距与最终宽度检查；参与者显示 label 另按 24 列限制。
- body limit 至少 8 且标题不裁剪，最终框可超过预算。
- 当前没有总行数或格数 cap；提示不创建图片或可点击打开行为。
- 两个 required-features binary 不开放。
- 并排显示 full 与逐字符 streaming 的当前 view，只比较纯文本行，不比较样式、链接、映射且 streaming 不调用 finish。
- 重新编辑或调整宽度 1/5；最小宽度按原文 pipe 数估计并至少 10，不是 parser 列数。
- 进入编辑、轮换 13 个 sample 或调整宽度 2；初始 sample/width 从 MERMAID_SAMPLE/MERMAID_WIDTH 读取，默认 0/70。
- 按含尾空白 token 分块；只有 plain-URL benchmark 明确 finish，不将其余结果当完成态。
- 没有自动阈值断言；本次仅阅读而未运行性能测量，不能据 fixture 得出性能保证。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 逐次阅读记录（历史阶段状态，最终结果见末尾）

# markdown 逐包审阅（进行中）

已完整阅读 Cargo.toml、src/lib.rs、src/parser_policy.rs。其余模块、playground与bench待逐一阅读，status保持pending。

- 默认features为空，playground启用可选crossterm/ratatui-textarea并开放两个测试binary；bench为criterion harness=false。公开入口包括ANSI、ratatui全输出、简化lines/map、复用buffers、table宽度和StreamingMarkdownRenderer。
- 一次性入口先normalize LaTeX，parser offset/source map属于规范化后的空间；内部streaming tail不重复normalize。full ratatui后处理plain URLs并按line/column排序，ANSI入口没有同一个post-scan调用。不能只据lib注释宣称全部输出模式完全等价。
- parser_options开启GFM、strike、math、tasklist、table；DoubleTildeOnlyStrike将单tilde起止事件降为literal text，双tilde保留删除线。四项内嵌测试检查单/双/混合/嵌套平衡，尚未运行。
- 公开模块checkpoint/streaming/style，其他模块通过re-export提供部分API；cfg(fuzzing)才导出test_syntect。lib示例ignored且依赖伪stream/theme，不能当编译用例。

## 关联独立包 markdown-fuzz

读取其manifest、target及README：实际仅pretty true/false×full/streaming共4路径，syntect恒None；streaming按1/16/32字节轮换并向后调整UTF8边界，不调用finish、不比较输出。README声称8组合且char-by-char与代码不符。待审种子、运行边界后为独立包建映射。清单source_lines=0不代表无源码，实际fuzz_targets/render_all.rs存在，后续校正统计。

## output/source_map/buffers 阅读

- output.rs604行、source_map.rs141行、buffers.rs390行已阅读，首次输出截断部分已补读。HyperlinkTarget使用display-cell列区间、u32逻辑ID、URL和Explicit/Inferred来源；共享ID支持分行片段分组，不是每fragment独立链接。
- MarkdownRenderOutput持有lines、line_source_map、hyperlinks、code_blocks，clear保留Vec容量；as_view为借用slice，line_count只读lines长度，公开字段不强制长度/坐标一致。
- CodeBlockSpan为closed fenced block，body为去容器标记、CRLF规范化内容；source_byte_range切parser输入字节，入口已LaTeX规范化时不能称原始用户字节坐标。output_line_range为pre-wrap正文，不含delimiter；Mermaid正文范围对应diagram输出，不等于源码行数。
- build_code_block_spans借单调newline cursor和partition_point查line_source_map；依赖meta与映射有序，meta顺序仅debug_assert，位置夹到text长度。空body锚定空range；源码注释每源行恰一输出行不适用于Mermaid替换，实际采用映射范围。
- span测试覆盖pretty/raw、open/closed、list/quote、缩进非fence、空body、无info、tilde、无尾换行、空行、CRLF、UTF8、内嵌短fence、tab、Mermaid干净源码和stream freeze/finish等价。无高亮条件测试；streaming_open_fence允许无span而循环空，不能独立证明正文冻结策略。
- SourceMap.add按source长度创建等长rendered段，无倒置/溢出校验；to_source对所有相交段取min start/max end，即使source不连续仍返回包围范围，与注释不连续返回None不符。它不验证rendered查询完整覆盖，不能作为无损精确copy证明。extend offsets直接相加；truncate/clear保留容量。ratatui output当前仅line map，SourceMap主要ANSI接口。
- buffers保存highlight/replacement/link/table/Mermaid中间数据与scratch；clear仅清外层Vec，嵌套字符串/Vec随元素drop，不等于完全无分配。Transform.force要求等长UTF8由消费方panic约束；table cell保存bold/italic/code/link来源，table hyperlink坐标为表局部cell范围，line_source_offsets单独映射border/wrap后的输出。floor/ceil UTF8边界函数夹到len并逐字节调整；RenderEvent派生排序为pos/kind/index/is_end，kind顺序Highlight/Replace/Table/Mermaid。
- 尚未运行markdown动态测试，其余parser/render/streaming与math等模块继续阅读。

## style.rs303行、colors.rs451行完整阅读

- TableBorders为11字符表，BOX默认，ASCII/DOUBLE与自定义构造；未验证字符display width，因此宽字符自定义边框不保证布局。MarkdownStyle派生Default是空Style，test_style只在test/fuzzing可见，与生产默认不是同一主题。
- adapt逐字段调用adapt_style；该函数重建fg/bg/effects但不复制underline_color，不能宣称完整保留全部style颜色。all_hidden要求至少一项且每项Some HIDDEN，None使false；merge按顺序覆盖颜色，HIDDEN在下一轮恢复prev，最终剥离HIDDEN，DIMMED/BOLD互斥且同项同时含二者时BOLD处理在后。
- detect OnceLock缓存首次结果，NO_COLOR只要存在（含空）就None；supports_color stdout返回None反而默认TrueColor。已知TERM_PROGRAM归一化或terminal环境变量存在可将低级别升TrueColor；不实测终端能力，后续环境变化不重探。
- COLOR_LEVEL_CAP与POLARITY_SAFE_SYNTAX是进程级Relaxed atomic；get_color_level取探测与cap最小。adapt_color先检查polarity开关并早返，该路径不检查NO_COLOR或cap，不能保证全局None时无色。
- polarity近灰chroma<40继承默认色，否则整数hue映射六种base ANSI；ANSI black/white/brightgray继承、bright accent降base，256先按xterm近似RGB。常规降级使用anstyle_lossy和VGA palette。透明背景也经同一adapt_color处理，不限语法前景token。
- colors八项测试包括基本调色板/顺序/部分polarity样例；flag测试直接改全局无锁，失败时不恢复。没有NO_COLOR×polarity、cap×polarity、underline_color保持或环境OnceLock重探测试。style无独立内嵌测试。尚未执行本包动态测试。

## syntax.rs 与 open_fence_highlighter.rs439行完整阅读

- Syntect::new解析tmTheme失败panic，加载two-face extra_newlines；theme/syntax_set公开可修改。按文件只取UTF8 extension，不读取文件首行或内容检测。fence info先识别数字:数字:path，再按扩展名查语言，否则整个info作token。
- citation只检查前两段非空ASCII数字，不转数、不验证正值/顺序/溢出；第三段可含冒号，Windows盘符在第三段不会被splitn继续拆。syntax_highlight_raw逐LinesWithEndings高亮，任何行失败返回None丢弃本次全部结果，未知syntax/无syntect也None；无size/time限制。五项测试仅基本citation/token解析与Rust可识别。
- OpenFenceHighlighter按body_reaches_eof选open增量或closed memo，closed key为info+全文，不含theme/syntax身份，调用方必须在主题变化时重建。命中仍clone全部span文本，不是零复制。
- closed memo预算256KiB只计body key字节，不含info和输出span内存。超限先clear然后仍插入当前body，因此单个超256KiB条目可使缓存超过预算；注释永不无界不成立。现有overflow测试各body约四分之一预算，只测试多条累计淘汰，不覆盖单超大条目。
- open按info/start/已提交前缀不匹配重建；以换行提交parse/highlight状态，尾部未换行在clone状态上试算。parse错误清info，下次通常重建；空info sentinel在相同空info/start0初态不强制重建，依赖调用方语言路径。
- 每次先扫描全部已缓存span比较prefix，返回clone全部已提交行；只有syntect已完成行不重复parse，整体stream仍可能二次成本和线性增长内存。无完整cache字节上限；主题对象换而未reset可能混色。
- 八项cache测试覆盖逐prefix YAML、info变换、start变化、未知语言、closed key幂等/区分/open交错、累计预算淘汰。start变化测试同时改变body，不能独立证明start-only触发；无同位置非append改写、主题交换、parse错误注入或单body超限测试。均尚未动态执行。

## checkpoint 完整与 streaming.rs1–490行阶段

- Checkpoint公开source_bytes/output_lines/kind，不自行验证稳定性；八类边界的实际生成仍须parse核对，不能把顶层注释当全部输入不变保证。
- Streaming保持规范化source、统一output、FrozenState、buffers、style/pretty/table width/softbreak、高亮cache和normalizer；source未包括normalizer暂存歧义尾部。Debug仅长度/模式，不输出正文。
- Clone重建source并render(None)，复制width/softbreak及normalizer状态，未复制Syntect，不能保证高亮输出等同；normalizer暂存另clone不重复规范化。
- set_style无条件清output/frozen/cache，其他setter仅值变化时清，不立即render。clear清source/output/frozen/normalizer、高亮cache并把width设None，但保留style/pretty/softbreak且未clear scratch buffers，不等于new全部默认。
- rerender先截到frozen，保留冻结行链接和end<=frozen的code span；冻结末尾无LF且tail起LF时跳一字节。tail links偏移line，code spans偏移line与source bytes；line_source_map直接extend未加源行offset，需结合parse/消费者进一步核对。
- 每render扫描新tail裸URL并将所有hyperlinks排序，checkpoint提交post_scan_next_id；该ID包含本轮尚未冻结尾部已分配值，不能仅据注释推导连续无间隙。None syntect不清旧open cache；换主题需调用set_style而非只换参数。
- 冻结前source仍完整保存、链接retain和sort涉及已冻结内容，不能把注释约O(N)提升为全入口复杂度保证。后续从491行开始完成finish等方法和测试。

## streaming.rs491–1300行阶段

- finish刷新normalizer暂存，使用新buffers与link_id0重渲染全部source（保留width/softbreak），重新URL扫描排序，将全部输出与source标冻结，释放高亮cache。into_output只取现有output，不flush或render；finish_into_output才执行完成流程。finish不消费self，之后还能push，不保证追加时原末段无需重算。
- 前8个blank-line测试仅cfg(test) helper，不是生产冻结决策。basic只核空/行数/冻结非零，clear测试检查width重设触发reset。finish等价测试只比较纯文本，不含所有style/link/map。
- 测试明确展示未finish的尾inline-code反引号被normalizer保留，source也缺最后字节；finish才恢复完整文本。所以push_and_render收到完整消息不等同完成协议。
- comprehensive char-by-char helper实际只push每字符然后render一次，不触发每字符checkpoint；line_source_map_with_checkpoints同样只最终render，不能验证多次冻结后的绝对行号。变量chunk helper才每chunk渲染但只比较lines，不比map/links。错误消息text[..min(50)]未修UTF8边界，失败时可能二次panic。
- 测试涵盖block/nesting/inline/basic edge、softbreak默认空格与关闭保留行map/style、heading style、demo行数和table无尾空行；多处push自动render注释过时。1300行停在demo ending测试中，后续继续。

## streaming.rs1301–2110行阶段

- 完成demo尾空行、heading/paragraph间距、双至四LF/空白分隔、code/table/list/blockquote/thematic混合的实际逐chunk文本比较；这些helper只比纯文本行，不校验map、链接或样式。
- 名为syntax_highlighting测试两条路径都传None，只比较span数量，不能证明syntect高亮。blockquote前缀测试断言单层“│ ”与嵌套“││ ”；thematic无尾LF跨chunk有明确回归。
- demo4char chunks实际逐chunk渲染并比较纯文本；文档中O(N)和毫秒数字只是fixture文字，无性能断言。
- edge2way遍历全部UTF8合法单切点；4way遍历主切点后仅取两半中点，不穷举全部四分组合。都pretty且syntect None，只比较末态文本，错误preview部分byte切片未校验UTF8。
- 小文档全部切点、blockquote全部chunk size提供实际多次render文本等价，但不是所有输入/样式/链接/映射保证。编号列表测试读至2110仍在构建renderer，后续从2111继续。

## streaming.rs 2111–2910 行补记

- 上轮已读完流式模块，补齐记录。编号列表逐 chunk 测试比较末态纯文本；URL byte-by-byte 测试只在全部输入后、finish 前后断言数量和 URL/行/列，不逐个中间 chunk 断言，也不比较 provenance。split Markdown URL 期望两个不重叠目标（标签和展示后缀）。重复 render 检查 ID/URL/位置；名为 monotonic 的测试实际检查集合唯一性，不能推导所有片段 ID 唯一（换行片段共享逻辑 ID）。
- 真实 Syntect 测试覆盖 YAML 长块、UTF-8、CRLF 和多种 chunk 大小。单个无冻结 open block 比较 lines 与 line_source_map；跨冻结 helper 明确注释 line_source_map 为 tail-relative，故仅比较 lines。该限制与生产代码直接 extend tail map 一致。
- style reset 测试沿用同一个 Syntect，仅改 code_untagged bold 并检查缓存清空，不是更换 Syntect theme 的验证。数学多种形式遍历合法双切点，比较纯文本；部分小 chunk 测试只比较 finish 后结果，不能排除此前中间态缺陷。clone 数学测试不含 Syntect。

## hyperlinks.rs 与 url_scan.rs 阅读阶段

- 已完整读取 url_scan.rs 649 行；hyperlinks.rs 本轮一次大输出发生截断，不能据此标记全文完成。已确认生产实现 1–203 行及另行补读 390–580 行，后续仍需补读测试区，防止遗漏。
- source_to_chunk_offset 假定 transform 按源位置排序；完整覆盖的替换按目标长度换算，部分跨端点在 debug 触发断言，release 将位置夹到替换起点。chunk_link_offsets 按源范围求交，假定 LinkTarget 源顺序并在起点越过 chunk 时提前退出，raw 模式忽略 transform。
- emit_segment_hyperlinks 将 transformed 字节边界向外对齐 UTF-8，再按 unicode display width 映射列；跨段复用 URL、ID 与 provenance，不负责排序或合并片段。零宽内容可能得到零宽列区间，不能等同每个目标至少占一个显示单元。
- plain_url_ranges 使用 OnceLock LinkFinder 且仅 LinkKind::Url，返回原输入 UTF-8 字节范围。Unicode 空白、零宽空格和指定中文句号/括号/引号分隔 prose。中文逗号/顿号/分号/冒号在 path 中分隔，在 query/fragment 内保留，除非后接 HTTP(S) URL 或位于末尾；ASCII 逗号/分号仅在非 query/fragment 且紧接 HTTP(S) 时分隔。显式 Markdown destination 不经过这些推断规则。
- detect_plain_urls_with_offset 先拼接一行的全部 style spans，因而可跨样式检测 URL，但不跨输出行；所有样式均参与，包括 inline code 与 fenced code。列按显示宽度计算，行加调用方 offset。与已有及本轮新增目标的任何同一行半开区间重叠都会抑制候选，与 URL 是否相同无关。新增 provenance=Inferred，u32 ID 普通自增，无显式溢出处理。每个候选遍历 existing/result，不能把候选拆分单遍算法当整个扫描线性复杂度。
- URL 测试覆盖中文和 Unicode 分隔、query 数据、大小写 scheme、500 个邻接 URL 的结果、跨 style spans 的 CJK 列、显式 destination 保留、entity 分隔、代码内 URL、后缀重扫与重置。500 项测试无时间断言。all-split 流式测试比较完成输入后但未 finish 的 URL/列与 finish 基准，未比 ID/provenance/行。reset survival 只比较数量与选定后缀位置，不证明全部 metadata 不变；pretty toggle 在两次 setter 之间未渲染 raw 态。
- 本轮没有运行 markdown 动态测试；包仍 pending，不能将读到的测试断言计为测试通过。

## hyperlinks 全文补读完成；LaTeX 转换核心完成

- hyperlinks.rs 测试区按 203–390、390–580、580–810、810–1056 区间补齐，全文现已读取。覆盖短 destination 展示、长文件/URL compact、Windows drive、含空格路径、命令不当文件、entity 顺序、描述标签不缩写、表格链接与样式、同名不同目标、CJK 宽度、softbreak 共享 ID 片段。长文件双 chunk 测试在 finish 后比较完整 lines/hyperlinks；多数 streaming 测试也先 finish，不能证明所有增量中间态正确。parser_link_text helper 按显示切片选目标，没有按 provenance 选择。零宽字符 helper 注释明确未覆盖组合字符边界。
- latex/ 的 mod、cursor、math_box、commands、environments、symbols、tests 全文完成。inline/display 入口以 UTF-8 字节长度 4096 为上限，超限 None；inline trim 每行、过滤空行后以分号空格拼接；display 仅 trim_end 并过滤空行，保留布局缩进，空内容返回 Some(empty)。调用方是否回退待 parse/render 核对。
- Cursor 按字符移动字节位置，命令名仅 ASCII 字母或单字符，保留命令后空白。group 支持转义花括号，不平衡时读到末尾。atom 可为 group、命令名或一个字符。MAX_DEPTH=32 只在 render_sequence 的花括号递归分支检查，达到后输出 raw group body；render_atom/command/environment 并非统一入口限深，不能把注释当所有调用路径的 32 层保证。
- sequence 丢弃无匹配右括号、游离 dollar 和环境外 alignment marker；数学模式将减号/单引号改为 Unicode minus/prime，text 模式保留；空白折叠但 tilde、quad、qquad 有单独处理。未知命令保留裸名称；label/tag/end 消费并丢弃花括号参数，大小和多种结构提示不输出。
- 上下标只在全部字符可映射且非 wordlike 时使用 Unicode。源含指定 text-family 标记或渲染值含连续 3 个 ASCII 字母时回退普通 ^/_，多字符加括号；这是一项启发式。分数使用固定 18 个 vulgar fraction 映射，其他输出斜杠并按有限运算符集合加括号；缺分母仅保留分子。binomial 为 C(n, k)，根号支持 2/3/4/其他上标索引，多字符被开方项加括号。
- boxed 保留数学模式内容，fbox/framebox/text-family 切 text；alphabet 仅映射有 Unicode 字形的字符，其余保留。accent 给每个非空白字符添加 combining mark，非整个 grapheme/单一重音布局。not 对有限关系直接替换，否则给最后输出追加 overlay。overset/stackrel/underset 仅在装饰可全部映射为 script 时保留装饰，否则静默省略，测试明确将 overset{!}{=} 渲染为等号。
- 环境名称 trim 并移除尾星号，同名 begin/end 深度匹配，缺 end 消费到末尾。array/alignat 丢弃首个花括号列配置。顶层 row/cell 切分避开花括号和环境深度，所有 cell 先 flat 渲染，故嵌套矩阵不保留内部二维盒。矩阵按显示宽度补列，行长不一时不为缺失末列补全；vmatrix/Vmatrix 都单竖线。cases 用左括号列；未知环境与 aligned 等同走行拼接并折叠双空格，不实现一般 TeX 对齐。
- MathBox 使用上中行锚定二维内容，按 display width 左补齐，后续文字接最大盒宽处；显式换行设 floor，后续盒不修改已完成行。inline 模式行断点为分号空格。
- 测试读取覆盖常见命令、script 启发式、二维锚点与换行、未知命令/环境、20 种 malformed fixture、200 层花括号和超长输入；不构成任意输入 never-panic 证明。已启动 cargo test -p markdown latex::tests，日志 /tmp/grow-markdown-latex-tests.log，结果待进程完成。
- mermaid.rs 只读到 210 行。入口实际尝试 graph/state/class/ER/sequence，顶部注释遗漏 class/ER；blank 返回 None，解析不支持/画布超限回退，宽度超限另带 too_wide 标记。具体解析和布局仍待读，不据常量推导所有路径上限。markdown crate 仍 pending。

## LaTeX 定向测试结果与 delimiter 阶段

- cargo test -p markdown latex::tests -- --test-threads=1 退出 0：44 passed、0 failed、0 ignored、458 filtered out。仅为本机默认特性下 LaTeX 转换测试，不覆盖 delimiter、Mermaid 或全包其余测试。
- latex_delimiters.rs 已读取 1–420 行。push 空字符串不刷新 pending；非空将 pending 与 chunk 合并处理。finish 对 pending 使用 final_flush 再运行状态机，尽管方法注释写 literal，实际仍可能将未配对反斜杠分隔符转换为 dollar；finish 不 reset code state，reset 才回初始状态。
- inline math opener 找到 close 后 ASCII 内侧 trim 并合并行；空 interior 只先输出 opener，继续后续字节流程；过远/未闭合则输出单 dollar 后普通处理，等待时不消费 opener。display opener 可来自反斜杠、equation 或两个 dollar；连续 dollar 每次只消费两个，single dollar 尾端会等待以判断是否形成 display。
- InlineCode 只匹配相同长度 backtick，遇换行直接退出 code state，即使后面还有配对 backtick；fenced state 按行扫描关闭。不能将头注释“仅 unmatched backtick 有区别”当一般 CommonMark 多行 code span 保证。fence scan 和 lookahead 细节在 421 行之后尚待审阅；暂不确认 pending 全路径有界性及任意输入 chunk-invariance。


## latex_delimiters 全文完成与定向验证

- 完成 421–1305 行，至此全文已读。fence opener 只允许 0–3 ASCII space、至少 3 个相同 backtick/tilde，不验证 backtick info 中是否含 backtick。close 要求同字符长度至少 opener，随后仅 space/tab 与 LF 或 final EOF；CR 不被接受。4 空格不识别 fence，容器前缀不处理。
- find_inline_close 按源字节距离检查 4096 上限，只接受未转义反斜杠右括号；不按段落中断。display 接受任意一种 display close，不要求与 opener 类型或 equation 星号一致；LF 后跳过空格/tab 遇 LF 或 > 放弃。多行 join 仅 trim space/tab/CR，保留非 ASCII 空白；单行 interior 原样输出。
- 全路径“bounded pending”不成立：开/闭 fence 尾部 run 无长度上限；关闭 run 后的 whitespace 等到下个非空白或 finish；display LF 后扫描至缓冲末尾空白时直接 NeedMore，未复查距离上限。4096 限制不足以给整个 normalizer 暂存定界。
- classify_backslash 对未识别转义仅消费反斜杠，下一字符仍普通处理；因此 escaped single dollar 的 fixture 不能推广到 escaped display dollar。代码 span 处理仅单行，不是 CommonMark 完整代码语义。
- 现有定向测试 cargo test -p markdown latex_delimiters:: -- --test-threads=1 退出 0：36 passed、0 failed、0 ignored、466 filtered out。包含 curated idempotency、合法 UTF-8 全双切点、逐字符、4000 个固定 LCG token soup；soup 明确不验证全输入幂等性，顶部普遍 idempotent 注释过强。这些有限 fixture 不证明任意输入 invariant。
- 用 rustc --edition=2024 编译独立探针，直接 path 引入本工作树真实 latex_delimiters.rs，唯一 stub 是同值 MAX_MATH_SOURCE_LEN=4096。没有修改仓库运行时代码。探针路径 /tmp/grow-markdown-normalizer-probe.rs，二进制 /tmp/grow-markdown-normalizer-probe。结果：
  - 输入 Rust 字面量 "```\r\nraw\r\n```\r\n\\(x\\)"，输出完全不变；LF 对照输出 "```\nraw\n```\n$x$"，证明 CRLF close 未退出 fenced state。
  - 输入 "\\$$\nx\n$$" 输出 "\\$$x$$"；转义反斜杠仍在，但 display 内容已合并。
  - 输入 "`a\n\\(x\\)`" 输出 "`a\n$x$`"，真实多行 backtick 内容发生转换。
  - 20000 个 backtick 输入：push 输出 0 字节，20000 字节待定；"```\na\n```" 后 20000 空格：20009 输入、6 输出、20003 待定；"$$\n" 后 20000 空格：20003 输入、0 输出、20003 待定。这里只核对 push 结果，不声称内存耗尽或 UI 端故障已复现。

## mermaid.rs 211–630 行阶段

- graph/flowchart header 大小写折叠，默认 TB，LR/RL/BT 映射其余 Down。先收集全部 statements，故节点上限不限制原始输入分配。subgraph 数量 24、栈深度 6 超限直接 None；未闭合栈仍接受，孤立 end 仅 pop。classdef/class/style/linkstyle/click/direction 被忽略，包括组内方向。node 上限 128，edge 512；组的已有节点再次出现不会自动迁移归属，重复显式 label 可更新形状。
- 分号仅在双引号外拆分，%% 双引号外截断；不处理反斜杠转义引号。A&B 到 C&D 生成笛卡尔积，左箭头且右非箭头时反转边。节点 ID 为 Unicode alphanumeric 或 underscore；不同 Mermaid 形状压到 Rect/Round/Diamond，缺 closer 取余下 label，解析不严格报错。
- label 清理先移除有限 HTML 格式标签、br 为空格，再去外引号；仅外层反引号字符串执行简化 Markdown 清理。实体最后单次解码，保留双重编码的下一层；仅五个命名实体及 decimal/hex 数值，数值 control 被拒绝。html_tag_at 扫到 > 不理解属性引号，不是完整 HTML parser。parse_link 从 630 行开始待审，布局和测试尚未完成。

## mermaid.rs 630–1820 行阶段

- flow edge 运算符接受连续 - . = < >，不验证标准 Mermaid 长度组合；= 优先 Thick、其次 . Dotted。首端 o/x 需后跟线字符，末端 o/x 需其后为空白、竖线、&、分号或 EOF，避免误吞目标 ID；label 支持立即 |...| 和无右箭头时的内嵌文本形式，缺结束 | 接受剩余文本。
- state header 使用 starts_with(statediagram)，非严格等值。direction 生效；note 多行至 end note 丢弃，单行冒号 note 丢弃；class/style 等忽略，复合状态不形成层级组。state quoted label 可用小写 as，choice 仅该小写 stereotype 变 Diamond；转移 --> 可链式，冒号后为 label，[*] 源/目标分别映射 start/end 两个键、共同显示 ●。无冒号且含空白的未知语句使整个 state parse None。
- class header 同样 starts_with；类块靠单独 } 关闭，EOF 未关闭仍接受。direction 支持四向，namespace 声明忽略，内部类平铺；无匹配语法返回 None。attrs/methods 按含 '(' 分类，各保留 8 条再添加一个省略号，后续丢弃；annotation 覆盖旧值，generic 的 ~ 交替替为尖括号而非解析嵌套泛型。
- class relation 支持继承/实现三角、组合实菱形、聚合空菱形、箭头/无箭头及实虚线，共固定 14 运算符，按最早字符位置和表顺序识别。双引号 cardinality 从两端抽出，与关系 label 拼为一个边标签。循环对每个字符调用 char_byte（从字符串起点 nth），不能承诺长单行线性解析。annotation/cardinality 不统一走 entity decode。
- ER header 严格等值（忽略大小写），relation 必须三个 whitespace token，中间运算符恰好 6 ASCII 字节，--/.. 与双字节基数分别转 0..1/1/0..*/1..* 文本；边不画 crow-foot 而合成标签。实体 alias 为 ID[label] 单 token，含空格标签不被一般声明接纳。属性忽略首个双引号开头 token 及后文（注释），保留 8 条加省略号。实体块缺 } 同样可接受 EOF。
- class/ER 共用 compartment renderer，标题含 annotation 和 generic 显示，字段/方法分区。图方向翻转最终 Canvas；Canvas::new 自身使用普通 w*h 且无上限检查，须继续核对各布局调用方，不能仅凭 MAX_CANVAS_CELLS 常量认定分配有界。
- Canvas blit 复制字符/分类/样式并标全子区域 occupied，不复制 mask；add_bits 对 occupied 不写线，junction 绕过该检查。finalize_mask 仅为空白字符填线，混合线样式退为普通线。左右翻转再将 Text/EdgeLabel 连续 run 反转以恢复阅读顺序，但 flip_glyph_h/v 对所有字符应用（包括标签中的箱线/箭头），并非严格文本内容不变；是否出现实际 fixture 差异待测试。翻转仅交换 ch/cls，适用最终绘制后的调用时序。
- to_lines 跳过宽字符 continuation 哨兵；plain 每行 trim_end，styled 按类别分 span，整空白行仍可保留空格 span，两者不保证字节等同。布局只读到 1820 行 box size 阶段；其余布局、sequence、fallback、测试待读。本轮未构建，缓存保持清理状态。

## mermaid.rs 1821–3670 行：生产实现完成

- layout_canvas 在完成 rank、尺寸、位置计算后，先比较 max_width，再用 saturating_mul 检查 2^21 canvas cells，然后才 Canvas::new。该限制为单画布而非全过程总内存；未限制解析 strings、尺寸向量、递归中同时存活子画布。Frame 子图尺寸扩外边框，class compartments 以最长完整字段决定宽度，不提前按 24 字符截断。
- self-loop 为节点保留 2 行并扩最小宽度到 7，label 按至多 28 显示列留空间。同一节点多条 self-loop 共用路径，后画覆盖前画；route_self 不读取 head_from，head_to=None 仍通过 head_glyph 默认得到箭头，属于当前表示限制，待测试阶段核对。
- rank 用显式栈 DFS 忽略灰色回边构成 DAG，最长路径分层；原始循环边保留，非相邻层边（包括跨层正向边）走侧通道。顺序依赖原节点/边插入次序，不承诺图同构规范布局。order_ranks 最多 8 轮重心排序，仅统计相邻层交叉并保留最佳结果；assign_positions 10 轮松弛保持节点间距，不保证全局最优。
- 轨道分配按端点区间排序，区间相隔至少 2 格或同源/同目标可共用。TD 侧通道在右，LR 在下；BT/RL 最后翻转。place_label 遇非空字符、mask 或 occupied 时立即终止，不另寻位置、不保证 label 完整。各 route 的箭头和交点可覆盖边框，self-loop 空间不足则直接不画。
- 普通 label wrap 宽度 24、最多 4 行，但先构建全部换行再 truncate，不能由输出限制推导内存上限。长 token 优先在 _ - . / 后断开，其余逐字符；空白被归一。fit_label 加省略号。draw_box 对 Diamond 和 Round 均使用圆角矩形，没有菱形外轮廓；按 char width max(1) 绘制，组合字符/零宽字符并非 grapheme-aware。
- grouped renderer 将与 group ID 同名的节点作为 proxy，跨组边提升到共同祖先作用域，并连接组边框；不承诺始终画到内部原端点。空且未引用、无有效子节点的组被移除。递归子 scope 传 max_width=None，各子图仍有 cell cap；只外层受宽度限制，外层失败发生在子图分配后。空 scope 返回 1×1 canvas。
- sequence 支持 participant/actor、隐式 participant、8 种实/虚箭头及 cross、self-message、单行 note over/left/right、autonumber。participant alias 只按带空格的小写 as 分割；未校验一般 ID 格式。autonumber 只设 bool，忽略起始值/步长/关闭参数，自 1 连续编号。activate/deactivate/create/destroy/title/链接元数据忽略；loop/alt/opt/par/critical/break 和匹配 else/and/option 为 Divider，rect/box 仅压 false 栈并不画区域。block 栈本身无独立深度限制。
- sequence 限 128 participant、512 message/note/divider；note 必须有冒号，over 至多取前两个 participant，left/right 只取首个，其余 ID 不使用。消息接受空白包围的任意非空源/目标，目标前 +/− 全部 trim，不实现 activation。空图无 participant 返回 None，只有 participant 可渲染。
- sequence 布局 participant label fit 24，正文/注释全文宽度决定间距，最短跨度需求优先扩大末 gap；每项固定行高，note 是三行框，无多行 note 布局。上下重复 participant 框，lifeline 穿过空白但避开 occupied。分配前同样先 width 后 cell cap；画 message text 使用 Text 样式、divider 用 EdgeLabel。
- fallback 不经 Canvas，全部源行 trim_end、跳过开头空行后收集并画框，没有源行数/总格数上限。max_width 的 body limit=max(width−4,8)，title 不裁剪，所以窄宽度不保证最终输出 <= max_width。仅 Oversize::Width 附英文打开图片提示；解析 None 或 cell 超限没有该提示。提示是文字，不在该函数生成图片/可点击入口。无 max_width 时 fallback 单行不截断，控制字符也不在该路径过滤。
- mermaid.rs 生产实现已全读，测试区刚读两项至 3670 行，余下至 5237 行待读。本轮无动态测试、无构建；markdown 尚未完成 parse/render 等模块，不标 reviewed。

## Mermaid 全文测试审阅完成

- 测试区补读 3671–5237 行，mermaid.rs 全 5237 行现已完成。当前仅源码审阅，未运行本包 Mermaid 测试；不能引用其他同名 mermaid crate 的 65 项结果替代本模块。
- 覆盖 HTML/Markdown label 清理、quote/分号/%%、实体单次解码、CJK 列与 sentinel、wrap/truncate/标识符边界、rank cycle、重心去交叉、不可避免交叉分轨、fan-out/merge 共头、back/skip lanes、反向及多种端点、自循环、class compartments 与成员 cap、ER cardinality、subgraph proxy/首定义归属/深度、state flat composite 与 choice 元数据、sequence notes/dividers/activation 忽略等。
- direct_push_sinks_decode_entities 明确指出无引号实体分号被 statement splitter 拆开，class member 和 ER attribute 的解码仅直接调用 finalizer 验证，不能认定完整 parser 接受其源码形式。quoted state/sequence label 有真实 parser 断言。
- er_entity_alias_label 仅用 plain 输出 contains(Person/Bank Account)，不排除 fallback 原文满足断言；parse_er 对该 relation 的 split_whitespace 得到超过 3 token，源码路径返回 None，故不能以此测试声称空格 alias 成功渲染。state_choice 测试检查 Shape::Diamond，而 draw_box 仍以圆角框显示；元数据断言不证明菱形外观。
- adversarial 10000 节点/单行链测试只检查 fallback 标头，深度 100 链只检查端点和箭头；没有峰值内存/时间上限断言。fallback 宽度测试使用 40 列，不能覆盖小于标题宽度的窄窗；over_wide 的一项断言仅 max_width<=src.len()，远弱于调用方 max_width。
- BT/RL 只核对普通词序，不覆盖含箱线/方向 glyph 的 label 内容。self-loop 测试仅箭头 loop，无无向或 head_from/多重 loop；这些源码观察仍待定向验证。sequence_rows_are_rectangular_and_sentinel_free 实际只检查 sentinel 不存在和 note 文本存在，未比较各行宽度。

## parse.rs 1–210 行阶段

- compact reference 阈值为 48 显示列（不是字节），含 LF/CR/*/反引号/方括号不 compact。file URL 经 URL 到本机 Path 再取 file_name；local reference 优先于 generic URL，保留 Windows drive 判定；HTTP(S) 取末个非空 path segment 或 host（不统一 percent decode）。inline code 仅明确 URL 或 anchored path；普通相对带斜线文本不走该入口。
- explicit label 先 HTML decode，仅 self-label、HTTP(S)/file、anchored path 或无空白且带斜线的强相对路径可 compact，描述标签含空格斜线保留。真实 destination 仍须后续主 parser 核对。
- normalize_transforms 按 start 升序/end 降序，让早起点最宽范围拥有字节；完全包含的后者丢弃，部分重叠及 force 差异仅 debug_assert，release 保留先前 owner。trim_reference_token 去有限左右标点，不通用路径语法。prose_reference_candidates 正在读 entity/source 处理，停在 210 行，后续继续。

## parse.rs 211–1390 行阶段

- prose reference 先裸 URL，再 whitespace token 中 anchored path，再成对单双引号路径；quote 扫描不理解 escape，候选重叠保留先找到者，最后源位置排序。源 URL entity decode 若引入新的边界则不 compact；后续 rendered scan 可再推断。hide destination 只看 local/HTTP(S) 与 >=48 列，不要求 reference_display_name 成功，因此 label 未缩写也可能隐藏长 destination。
- parser 内 anstyle→ratatui 映射只包含 fg/bg 和 bold/dim/italic/underline/strike/hidden；未传 underline color、reverse/blink 等效果，不等同全部 anstyle。find_substring allow_outside=false 只接受借用内容指针落在 haystack 范围内，true 才按文本 find/rfind，匹配分配/解码后的文本行为不同。HTML entity helper 用全 HTML5 decoder 并拒绝控制字符，与 Mermaid 的五命名实体表不同。
- cell_word_separator 只把 ASCII space 当 whitespace break，但 URL 保护 token 用 split_whitespace；punct 后 alphabetic 可断，digit-punct-digit 除逗号/句号可断。注释称 EMP-1001 可断与条件不一致：digit_before_break 为 false 时无该 digit 分支。URL::parse 成功的任何 scheme token 都保护，非仅 HTTP；标点归属按相邻显示宽度最小化，不能据此保证所有 URL 最终不会被 render 层强制折行（待后续核对）。
- MarkdownParser::parse 清理共享 buffers 后消费统一 parser_policy/TextMergeWithOffset 事件，再 normalize_transforms。starting link ID 与 open_fence 仅 crate 内注入；parser 自身不接收 pretty，收集替换供 renderer 选择。祖先 heading/strong/emphasis/strike 会叠样式；link 内 strong 等只去 fg，保留 bg/effects；heading fg 未去除。Link/Image 的 None ancestor 防止默认 text fg 覆盖。
- table Text 使用已解码事件内容做 compact 候选，独立 cell_link 及 inferred ID；结构链接则逐 Text chunk 缩写，并非总标签一次判定。table code 在 cell 标为 code 后 push；是否无结构链接时支持裸长 code path 的语义仍须看 style_inline_code_span 与 whole-table replacement，不能套用 prose 行为。
- fenced body 累积 clean text 与 raw range；Mermaid 触发条件是 info 首 token case-insensitive mermaid 且 Text range.end < source.len()，不是在此分支检查真正 fence closer。try_push_mermaid 成功提前返回，否则 Syntect（可使用 streaming cache）或 code_untagged，后者记录 untagged range。最终 closed span 判定仍在 on_end 待读。
- inline math 转换非空则 math style+pretty transform；table 内 italic，失败 code 内容。display math table 内强制 flat；外部走 block replacement，失败显示 code_outer/TeX 高亮。转换失败/空值的调用方回退现已核对，raw/pretty 最终选择仍须 renderer。
- SoftBreak 在 table 永远 push space；非 table 仅 collapse 开且下一字节不是 space/tab/>/| 才 force 替换；替换长度等于原 source span，CRLF 对应两个空格，注释“一空格”并非此层精确字节结果。HardBreak table push LF。HTML block 普通 text；inline br table LF、prose pretty transform LF；其他 inline HTML table 原文，非 table 有 Syntect 才 Replace，无渲染 DOM 行为。
- Rule pretty 替换为固定三横线，顶层设置 ThematicBreak checkpoint；task marker 仅 checked/unchecked style，没有此处图形替换。checkpoint nesting depth 计 BlockQuote/List/Item/Table，非所有 Tag。
- heading marker 扫描起始 #/space，级别夹到 1–6；blockquote 每行按深度寻找第 N 个 >，不额外验证它是合法 prefix（懒续行文本边界待测试）。code_outer 只在 fence 前缀全 space/tab 时向行首扩，保留结构 >。list Item 识别 -/* 加空格及数字 . 或 ) 加空格，只有 -/* 替为 bullet；+ 或 tab 分隔不在该 helper 的转换分支。
- table start/header/row/cell 状态与 emphasis/strong bool 已读取，strikethrough 此处仅全局 highlight，无 cell strike 字段更新。停在 Link/Image on_start 1390 行，后续继续；未运行新测试、未构建。

## parse.rs 全文完成（1391–2456）

- Link/Image 共用语义路径，table start 分配 Explicit ID、设置单个 cell_link 后提前返回；end 清为 None，不保存嵌套 link 栈。prose title 搜索双/单引号形式，destination 从 tag 源 rfind；仅找到 destination 才在前缀 rfind ]( 定位结构。解码后 destination 未能在原文匹配时走整 tag 外层样式/整范围 target，不能把注释“Owned 已处理”当所有 entity URL 坐标精确保证。
- 显式 inline link 仅非空 text range 分配 target；visible 非空白 label 且长 local/HTTP(S) 才隐藏 destination+尾部，otherwise ]( 变空格左括号。Image 使用相同文字/URL 处理，不在该模块下载或显示图片。长 autolink 有单独缩写；普通 autolink/引用式 fallback 将全 tag 范围作为 Explicit target。
- on_end 直接 pop tag stack；table italic/bold 用 bool 清零，不按嵌套计数恢复。CodeBlockMeta 仅 fenced pending body_range.end < end-event range.end 才产生；清 pending 与是否 closed 无关。table 完结格式化整范围并生成 TableReplace。
- checkpoint 只在 depth=0，paragraph/heading/code/blockquote/list/table/html；以 has_blank_line_after 或 code end 非 EOF 判定，code+blank 时 range.end+1（跳 1 字节，不是扫描后的全部 whitespace）。该条件与 CodeBlockMeta body-end 判定独立：闭合 fence 恰在 EOF 可能有 metadata 而不冻结；不得把任意 end event 等同闭合。has_blank_line_after 跳 space/tab，仅认 LF，不统一 CRLF。
- inline code 在 table_state 存在时不生成 compact path target，故表格无显式 link 的长 code path 与 prose inline code 行为不同。source inner 匹配先借用指针、再首个文本 find；失败全范围 inline_code_inner。
- Mermaid 使用 max_table_width 作为图宽度，rule/text/emphasis/strong 映为边框/节点/边标签/标题。display math 使用 TableReplace，两空格缩进、消费紧接 LF/CRLF，source offset 为 min(i+1,源 LF 数) 的近似映射，无 links。
- inline entity scan 最长 33 字节、仅 ASCII 名称/数字/hash/分号组成，拒绝 decoded control；若与任何既存 transform 重叠则不添加。遍历 entity 时反复扫描所有 transforms，单 entity lookahead 有界不代表整个入口严格线性。
- format_table 固定 TableBorders::BOX、padding=1、rule.dim 边框，不提供选择先前公开 ASCII/DOUBLE 常量的参数。列数取非空行最大值，自然宽度取每个 LF 分行的最大显示宽度。max_table_width 先扣 3*num_cols+1；最小列宽是自定义 separator 的最大不可拆词，默认至少 1。预算低于 min_total 时仍采用所有 minimum，因此注释“guarantees total never exceeds budget”不成立。超额预算按额外需求比例 floor 再按 unmet want 补 1。
- wrap_cell_text 使用 FirstFit、break_words=false，单词过宽溢出，不截词；width=0 则输出空。table header 全加 bold，code style 会替换先前 bold/italic，link style 最后 patch。左右/中心对齐按 saturating padding；无最大行数或源长度限额。
- 表格输出含顶部、header separator、每个 body row 后（非最后）divider 和底部；source offsets 顶部/header=0、separator=1、body=i+2、divider=对应 body、bottom=1+rows.len()。展开的多视觉行重复同一源行号。
- cell 样式/links 投影用之前 wrapped 行 byte 长度之和作为搜索下界，再 find 当前行文本，未累计已消费空白位置；重复文本可能错配到前段，当前仅源码疑点，需定向验证后才认定错误。底层 slice end 未额外 UTF-8 对齐，依赖 find/原 span 边界。记录风险而不改实现。
- ParsedMarkdown::new 为公开构造，允许调用方传 buffers/checkpoint/ID，不验证范围和排序；高层正常 parser 已建立部分前置条件。parse.rs 全 2456 行完成，测试主要在其他模块，不能凭阅读当动态通过。

## render.rs 1–240 行阶段

- anstyle→ratatui 与 parser 内同一有限效果集合，未统一复用。ANSI syntect replacement 去背景后 adapt，按非空 span 输出独立 reset，拼接依赖原 span 自带换行。StyledStr plain 无控制码，否则 style/text/reset。
- apply_transforms raw 模式只使用 force，pretty 使用全部；每个与 chunk 相交的 transform 都输出整个 replacement，不按交集裁剪目标文本，因此 parser/event 必须避免一个 replacement 跨多个输出 chunk；公开 ParsedMarkdown 构造可不满足该前提。遍历全部 transforms，没有局部索引。
- build_render_events 已读 highlight/replace/table 起止，后续排序/执行和 source map 仍待读。未构建。

## render.rs 241–1230 行：生产实现完成

- 所有 highlight/replace/table/Mermaid 起止事件统一 sort_unstable，执行依赖 RenderEvent 的 kind/index/end 排序及 parser 所建合法范围。ANSI 为 force transform 复制整个 source 并 copy_from_slice，长度/边界是 debug assertion，公开构造非法 buffers 在 release 仍可能 panic；不是容错外部格式。
- ANSI 普通文本按 highlight ID 的 BTreeSet merge，相邻同 style 源范围合并；pretty all_hidden 才跳过。纯换行与没有可见背景的 ASCII 空白清 style。普通非-force pretty transforms 从未 apply，故 bullet/entity/math-inline/reference 缩写等与 ratatui 不同，不能承诺两路径去 ANSI 后文本相同。仅 force 会先原位替换。
- ANSI Syntect Replace 在 raw/pretty 都执行，周围显式 reset，去 syntect 背景。SourceMap 对 replacement 使用原 source range 长度建立区间，但 rendered_offset 按实际 replacement 文本长度推进，长度改变时映射不等长。Table（含 display math）与 Mermaid 仅 pretty 输出 plain lines，不保留 table styled spans，增加 rendered_offset 但没有 source_map.add，插入换行也无映射。ANSI 不产生 HyperlinkTarget/OSC8（高层是否另加需调用方审阅）。
- ratatui 主文本按 active highlights Vec 的入场顺序 merge，与 ANSI ID 排序不同；非-force transform 执行。checkpoint split 先向后贴 UTF-8 边界，切分时 flush pending spans 再取 lines.len()；最终返回仍原 source_bytes，公开非法 checkpoint 不能依靠这一处贴边保证安全，末尾 fallback 的 source[..cp.min(len)] 仍直接 str slice。
- 隐藏内容只有行首 fence 前缀 ```/~~~ 才切换 in_hidden_code_block 并在上一行非空时插 separator；其他行首 hidden 也置 skip_leading_newline。skip 只处理 LF；跳过后 text_start 改变，但 apply_transforms 仍传 range_start，坐标偏移是否命中边界须定向测试，当前仅源码观察。
- ratatui 使用原始源 LF 扫描更新 source line，但 transformed segment.len() 也用于 byte_offset 推进；长度变化下映射为近似，不能写所有 transformed 行精确回源。links 用单调 next_link_idx，依赖源排序，投影至 display cells；Syntect replacement 本身不投影 parser links，高层 rendered URL scan 后续补裸 URL。
- Syntect Replace 在 raw/pretty 均用 code_background 覆盖原 syntect 背景后 adapt，输出整行；未标语言 code 借 untagged range 赋行背景。Table 先 flush pending inline spans，再发 styled lines 和局部 links 加输出行偏移；Mermaid 直接 push styled lines、全部 map 到 body 起始源行，不类似 table 先 flush pending spans，需保留结构前缀测试边界。
- 尾部无事件文本只 apply force，即使 pretty 也忽略普通 transform；pretty 丢全 ASCII 空白尾段，否则 raw span。最终 pending 行按产生 chunk 的 code membership 赋背景。fallback checkpoint 根据 source_line_map 前缀 < cp 源行的数量，cp>=EOF 冻结全部；不是每行完整 byte range 校验。
- 末尾用 mem::take(buffers.code_blocks) 建 CodeBlockSpan；重复对同一个 ParsedMarkdown 调 render_ratatui 会消耗 code metadata，不能由 &mut API 推导输出幂等，需针对公开入口验证。源码生产实现读完；测试区 1160 左右起已读至 1230，仅数个 Mermaid 与 link 回归，余下至 2754 待读。本轮未构建。

## render.rs 测试区 1231–2754：全文阅读完成

- 这些测试本轮只阅读，未运行。blockquote raw/pretty 有精确行断言；emoji/thematic fixture 仅无 panic。code separator 只检查索引距离，不能据此声称中间行为空；背景测试部分循环没有非空输出断言。
- 表格 30 列宽 fixture 检查宽度及矩形，但不能推广为所有最小列宽都服从预算。长 header 测试未逐词检查；multibyte fixture 主要检查 ASCII 内容存在。源码映射有范围断言，部分内容匹配条件可跳过，不能据测试名字承诺所有行精确映射。separator 测试确认 EMP-1001 保留整词；HTTP/FTP/SSH 与数字中的逗号、小数点有实际 fixture。
- inline table style 检查非 default，未完整验证所有具体样式。citation 使用真实 Syntect 比较 span 数，不是完整文本和样式等价。HTML/br 测试检验内容和部分换行条件，没有全面坐标契约。
- soft break LF 有单空格精确输出，CRLF 有两个空格精确断言；hard break 有两行和 [0,1] 映射。软换行映射测试只检查一项且值 <=1。代码内部换行、列表及引用 fixture 独立覆盖。缩进代码的 Syntect 测试比较去样式文本，不是每个 span 样式完全一致。
- math 测试覆盖 Unicode、行内/块、表格、引用、equation、希腊字母、script、aligned/cases、代码内保留和 oversized fallback 内容存在。emphasis fallback 测试仅检查特定文本存在/箭头不存在，不能证明回退分支；link-label display math 测试检查 URL 文本存在，不检查 HyperlinkTarget 元数据。unclosed fixture 仅检查三条入口无 panic。ANSI display math 检查块换行，不意味着行内变换与 ratatui 一致。
- entity 测试对 pretty prose 的 XML/HTML5 named、十进制/十六进制、NBSP、组合表达式有精确文本断言；raw 保留源实体，code 保留字面量，emphasis/heading/table 有回归。control fixture 实际枚举 ESC/BEL/NUL，注释提及 CR 但此列表未覆盖 CR。数学内 entity 只检查尾部及无重复，不检查数学表达式正确。link entity 只检查可见 URL。未知名称、无分号和 bare ampersand 有精确结果；UTF-8、畸形实体和 200 个 ampersand 仅无 panic，不是复杂度测试。

## 磁盘管理

- 按用户要求，本任务独立工作树 target 已清理；本轮复查为 8 KiB，文件系统约 77 GiB 可用。既有测试日志保留在 /tmp。未清理主工作树，未启动新 Cargo 构建。

## 全包完成记录（覆盖前述阶段的 pending 与未运行状态）

- 全部 28 个所属 Rust 文件（src、3 个 bin/helper、1 个 bench）及 Cargo.toml 已读完。嵌套 fuzz 是独立包，从本包 reviewed_files 和 source 统计排除；theme 另计入 SHA-256 证据。历史逐次记录保留以追踪审阅过程，不代表最终完成状态。
- 两个 playground 的共用 Tokyo Night plist 可解析，115 个 settings（包含全局项），name=Tokyo Night、license=Apache-2.0；程序通过 OnceLock 加载真实 Syntect。该资产不是 MarkdownStyle::default 的默认主题。
- md-table-test：初始 24 列、编辑态，pipe 原文计数估计 min width=max(4k+1,10)，并不解析转义或实际表列。full 与逐字符 streaming 均 pretty+Syntect，但 streaming 不 finish；差异只比较行数与纯文本。正文编辑实时重算，mouse 仅编辑态处理；Ctrl-Q 全局退出，Esc/Tab 仅编辑态离焦，查看态 Space/Enter 重新编辑，h/l 与 H/L 调 1/5。顶部旧注释所写 Esc 总退出、修饰 Enter 提交与实际分支不同。
- md-mermaid-test：13 份 sample 覆盖五种图及 groups/cycles；MERMAID_SAMPLE parse 失败默认 0 并模 sample 数，MERMAID_WIDTH 默认 70、初始值不 clamp，后续调宽至少 10。查看态 Tab 编辑、n 轮换、h/l 调 2；Esc 只在编辑态离焦，并非通用 toggle。没有 mouse capture/处理，也不启用 bracketed paste 事件路径。
- 两程序只在正常退出路径逐步恢复 terminal；前面任一 ? 出错可能绕过 cleanup。渲染宽高 cast/sum 使用 u16，过大输入或环境宽度没有一般溢出防护；panel 实际宽度受终端 area 限制，wrapped_line_count 只是显示宽度除法近似而非 Paragraph 实际词折行。交互本轮未运行，编译通过不能证明终端还原或视觉行为。
- Criterion 注册 10 组函数，覆盖常规 full、链接 full/stream、数学 full/stream、一般 full 重渲染和 incremental、裸 URL finish、open YAML（100/500/1081 行）及 closed Scala fence-in-list 对照（200/400/800 尾词，sample_size=10）。token 用 split_inclusive whitespace，不是一般字符/字节流。普通各 incremental 不 finish；裸 URL 才 finish。fixture 注释的 O(N)、历史 ms 和 constant bound 没有自动性能判定，本轮未跑 benchmark。
- 2026-09-07 在本独立工作树执行 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p markdown --all-features --target-dir /Users/lordcasser/workspace/projects/grow-openspec-sdd/target -- --test-threads=1：退出 0，502 单测通过、0 失败/忽略，两个 playground binary 各 0 测试，3 个 doctest 全部 ignored。日志 /tmp/grow-markdown-all-features-tests.log；早先 44/36 个定向测试为子集，不重复累加。
- 测试后立即 cargo clean --profile dev --target-dir 本任务 target，退出 0，删除 1572 文件约 407.9 MiB，未清理 main 工作树。这个测试覆盖本包 Mermaid 模块，不引用另一 mermaid crate 的结果；不覆盖 libFuzzer campaign 或交互 UI。
- 最终复核 src/lib.rs：ANSI render_markdown 也调用 normalize_latex_delimiters。与 ratatui 的差异是输出变换和 URL metadata 后扫描，不能误写成 ANSI 未做规范化。
